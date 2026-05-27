use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use bytes::Bytes;
use futures_core::Stream;
use http_body_util::{BodyExt, Full, StreamBody, combinators::BoxBody};
use hyper::body::Incoming;
use http_body::Frame;
use hyper::header::{HeaderName, HeaderValue, CACHE_CONTROL, CONNECTION, CONTENT_TYPE};
use hyper::server::conn::http1;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

use crate::config;
use crate::error::{AppError, AppResult};
use crate::models;
use crate::paths::{codex_auth_path, DEFAULT_HOST};

type RespBody = BoxBody<Bytes, AppError>;
type StreamResult = Result<Frame<Bytes>, AppError>;

#[derive(Default)]
pub struct EmbeddedShimState {
    server: Mutex<Option<ServerHandle>>,
}

struct ServerHandle {
    port: u16,
    settings_path: PathBuf,
    shutdown: Option<oneshot::Sender<()>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedStatus {
    pub running: bool,
    pub port: u16,
    pub settings_path: String,
    pub message: String,
}

#[derive(Clone)]
struct ServerContext {
    settings_path: PathBuf,
    client: reqwest::Client,
}

#[derive(Debug, Clone)]
struct RouteModel {
    slug: String,
    model: String,
    provider: String,
    base_url: String,
    api_key: String,
    extra_headers: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct ToolCallState {
    id: String,
    call_id: String,
    name: String,
    arguments: String,
    output_index: usize,
    closed: bool,
}

#[derive(Debug, Clone)]
struct ReasoningState {
    id: String,
    text: String,
    output_index: usize,
    closed: bool,
}

struct ResponsesStreamState {
    response_id: String,
    message_item_id: String,
    model: String,
    message_index: Option<usize>,
    message_text: String,
    message_opened: bool,
    message_closed: bool,
    tool_calls: HashMap<usize, ToolCallState>,
    reasoning: Option<ReasoningState>,
    next_output_index: usize,
}

struct MpscBodyStream {
    rx: mpsc::Receiver<StreamResult>,
}

impl Stream for MpscBodyStream {
    type Item = StreamResult;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

impl EmbeddedShimState {
    pub async fn start(&self, settings_path: PathBuf, port: u16) -> AppResult<EmbeddedStatus> {
        {
            let guard = self.server.lock().unwrap();
            if let Some(handle) = guard.as_ref() {
                return Ok(EmbeddedStatus {
                    running: true,
                    port: handle.port,
                    settings_path: handle.settings_path.display().to_string(),
                    message: format!("Embedded shim already running on http://{DEFAULT_HOST}:{}", handle.port),
                });
            }
        }

        let addr: SocketAddr = format!("{DEFAULT_HOST}:{port}")
            .parse()
            .map_err(|err| AppError::msg(format!("invalid listen address: {err}")))?;
        let listener = TcpListener::bind(addr).await?;
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        let ctx = Arc::new(ServerContext {
            settings_path: settings_path.clone(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(600))
                .build()?,
        });

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { break };
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            let service = hyper::service::service_fn(move |req| {
                                let ctx = ctx.clone();
                                async move {
                                    let response = handle_request(req, ctx).await.unwrap_or_else(error_response);
                                    Ok::<_, Infallible>(response)
                                }
                            });
                            let io = TokioIo::new(stream);
                            let _ = http1::Builder::new()
                                .serve_connection(io, service)
                                .await;
                        });
                    }
                }
            }
        });

        let mut guard = self.server.lock().unwrap();
        *guard = Some(ServerHandle {
            port,
            settings_path: settings_path.clone(),
            shutdown: Some(shutdown_tx),
        });
        Ok(EmbeddedStatus {
            running: true,
            port,
            settings_path: settings_path.display().to_string(),
            message: format!("Embedded shim started on http://{DEFAULT_HOST}:{port}"),
        })
    }

    pub fn stop(&self) -> EmbeddedStatus {
        let mut guard = self.server.lock().unwrap();
        let Some(mut handle) = guard.take() else {
            return EmbeddedStatus {
                running: false,
                port: 0,
                settings_path: String::new(),
                message: "Embedded shim is not running.".to_string(),
            };
        };
        if let Some(tx) = handle.shutdown.take() {
            let _ = tx.send(());
        }
        EmbeddedStatus {
            running: false,
            port: handle.port,
            settings_path: handle.settings_path.display().to_string(),
            message: "Embedded shim stopped.".to_string(),
        }
    }

    pub fn status(&self) -> EmbeddedStatus {
        let guard = self.server.lock().unwrap();
        if let Some(handle) = guard.as_ref() {
            EmbeddedStatus {
                running: true,
                port: handle.port,
                settings_path: handle.settings_path.display().to_string(),
                message: format!("Embedded shim is running on http://{DEFAULT_HOST}:{}", handle.port),
            }
        } else {
            EmbeddedStatus {
                running: false,
                port: 0,
                settings_path: String::new(),
                message: "Embedded shim is stopped.".to_string(),
            }
        }
    }
}

async fn handle_request(req: Request<Incoming>, ctx: Arc<ServerContext>) -> AppResult<Response<RespBody>> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    match (method, path.as_str()) {
        (Method::GET, "/health") => health_response(&ctx).await,
        (Method::GET, "/v1/models") => models_response(&ctx).await,
        (Method::POST, "/v1/responses") => responses_response(req, &ctx).await,
        (Method::POST, "/v1/chat/completions") => chat_completions_response(req, &ctx).await,
        _ => Ok(text_response(StatusCode::NOT_FOUND, "not found")),
    }
}

async fn health_response(ctx: &ServerContext) -> AppResult<Response<RespBody>> {
    let models = load_models(&ctx.settings_path).await?;
    let auth = config::read_codex_auth(&codex_auth_path()).await?;
    Ok(json_response(
        StatusCode::OK,
        json!({
            "ok": true,
            "models": models.len() + usize::from(auth.passthrough_available),
            "chatgpt_passthrough": auth.passthrough_available,
            "embedded": true
        }),
    ))
}

async fn models_response(ctx: &ServerContext) -> AppResult<Response<RespBody>> {
    let models = load_models(&ctx.settings_path).await?;
    let auth = config::read_codex_auth(&codex_auth_path()).await?;
    let now = now_secs();
    let mut data = Vec::new();
    if auth.passthrough_available {
        data.push(json!({"id": "gpt-5.5", "object": "model", "created": now, "owned_by": "chatgpt"}));
    }
    data.extend(models.into_iter().map(|model| {
        json!({"id": model.slug, "object": "model", "created": now, "owned_by": "codex-shim"})
    }));
    Ok(json_response(StatusCode::OK, json!({"object": "list", "data": data})))
}

async fn responses_response(req: Request<Incoming>, ctx: &ServerContext) -> AppResult<Response<RespBody>> {
    let body = read_json(req).await?;
    let requested = body.get("model").and_then(Value::as_str).unwrap_or_default();
    if requested == "gpt-5.5" || requested.starts_with("openai-gpt-5-5") {
        return Ok(text_response(
            StatusCode::BAD_GATEWAY,
            "ChatGPT passthrough is not implemented in the embedded Rust shim yet.",
        ));
    }
    let route = find_route(&ctx.settings_path, requested).await?;
    if is_openai_chat(&route.provider) {
        let forwarded = responses_to_chat(&body, &route);
        return post_openai_chat(ctx, &route, forwarded, true).await;
    }
    if route.provider == "anthropic" {
        return Ok(text_response(
            StatusCode::BAD_GATEWAY,
            "Anthropic embedded translation is not implemented yet.",
        ));
    }
    Ok(text_response(
        StatusCode::BAD_GATEWAY,
        format!("Unsupported model provider: {}", route.provider),
    ))
}

async fn chat_completions_response(req: Request<Incoming>, ctx: &ServerContext) -> AppResult<Response<RespBody>> {
    let mut body = read_json(req).await?;
    let requested = body.get("model").and_then(Value::as_str).unwrap_or_default();
    let route = find_route(&ctx.settings_path, requested).await?;
    if !is_openai_chat(&route.provider) {
        return Ok(text_response(
            StatusCode::BAD_GATEWAY,
            format!("Unsupported model provider: {}", route.provider),
        ));
    }
    if let Value::Object(map) = &mut body {
        map.insert("model".to_string(), Value::String(route.model.clone()));
    }
    post_openai_chat(ctx, &route, body, false).await
}

async fn post_openai_chat(
    ctx: &ServerContext,
    route: &RouteModel,
    body: Value,
    as_responses: bool,
) -> AppResult<Response<RespBody>> {
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let url = join_url(&route.base_url, "/chat/completions");
    let mut request = ctx
        .client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .json(&body);
    if !route.api_key.is_empty() {
        request = request.bearer_auth(&route.api_key);
    }
    for (key, value) in &route.extra_headers {
        if let (Ok(name), Ok(header_value)) = (
            HeaderName::from_bytes(key.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            request = request.header(name, header_value);
        }
    }
    let response = request.send().await?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await?;
        return Err(AppError::msg(format!("upstream returned {status}: {text}")));
    }
    if stream {
        return Ok(stream_openai_chat(response, route.slug.clone(), as_responses));
    }
    let payload: Value = response.json().await?;
    if as_responses {
        Ok(json_response(
            StatusCode::OK,
            chat_completion_to_response(payload, &route.slug),
        ))
    } else {
        Ok(json_response(StatusCode::OK, payload))
    }
}

async fn read_json(req: Request<Incoming>) -> AppResult<Value> {
    let bytes = req
        .into_body()
        .collect()
        .await
        .map_err(|err| AppError::msg(err.to_string()))?
        .to_bytes();
    serde_json::from_slice(&bytes).map_err(AppError::from)
}

async fn load_models(settings_path: &Path) -> AppResult<Vec<RouteModel>> {
    let file = models::read_file(settings_path).await?;
    let mut counts: HashMap<String, usize> = HashMap::new();
    for row in &file.models {
        if !row.model.trim().is_empty() {
            *counts.entry(row.model.trim().to_string()).or_default() += 1;
        }
    }
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut routes = Vec::new();
    for (index, row) in file.models.into_iter().enumerate() {
        let model = row.model.trim().to_string();
        let provider = row.provider.trim().to_string();
        let base_url = row.base_url.trim().trim_end_matches('/').to_string();
        if model.is_empty() || provider.is_empty() || base_url.is_empty() {
            continue;
        }
        let display_name = row
            .display_name
            .clone()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| model.clone());
        let slug_base = if counts.get(&model).copied().unwrap_or(0) > 1 {
            display_name.clone()
        } else {
            model.clone()
        };
        let mut slug = slugify(&slug_base);
        if used.contains_key(&slug) {
            slug = format!("{slug}-{index}");
        }
        while used.contains_key(&slug) {
            slug = format!("{slug}-{}", used.len());
        }
        used.insert(slug.clone(), 1);
        routes.push(RouteModel {
            slug,
            model,
            provider,
            base_url,
            api_key: row.api_key,
            extra_headers: map_headers(row.extra_headers),
        });
    }
    Ok(routes)
}

async fn find_route(settings_path: &Path, requested: &str) -> AppResult<RouteModel> {
    let models = load_models(settings_path).await?;
    if let Some(model) = models.iter().find(|m| m.slug == requested) {
        return Ok(model.clone());
    }
    let matches: Vec<RouteModel> = models.into_iter().filter(|m| m.model == requested).collect();
    if matches.len() == 1 {
        return Ok(matches[0].clone());
    }
    Err(AppError::msg(format!("Unknown model slug/model: {requested}")))
}

fn map_headers(headers: Option<Map<String, Value>>) -> HashMap<String, String> {
    headers
        .unwrap_or_default()
        .into_iter()
        .map(|(key, value)| {
            let rendered = value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string());
            (key, rendered)
        })
        .collect()
}

fn responses_to_chat(body: &Value, route: &RouteModel) -> Value {
    let mut messages = Vec::new();
    if let Some(instructions) = body.get("instructions") {
        let text = content_to_text(instructions);
        if !text.is_empty() {
            messages.push(json!({"role": "system", "content": text}));
        }
    }
    messages.extend(responses_input_to_messages(body.get("input")));
    if messages.is_empty() {
        messages.push(json!({"role": "user", "content": ""}));
    }

    let mut chat = Map::new();
    chat.insert("model".to_string(), Value::String(route.model.clone()));
    chat.insert("messages".to_string(), Value::Array(messages));
    chat.insert(
        "stream".to_string(),
        Value::Bool(body.get("stream").and_then(Value::as_bool).unwrap_or(false)),
    );
    copy_field(body, &mut chat, "temperature", "temperature");
    copy_field(body, &mut chat, "top_p", "top_p");
    copy_field(body, &mut chat, "max_output_tokens", "max_tokens");
    copy_field(body, &mut chat, "max_tokens", "max_tokens");
    copy_field(body, &mut chat, "parallel_tool_calls", "parallel_tool_calls");
    if provider_accepts_thinking(&route.provider, &route.model) {
        if body.get("thinking").and_then(Value::as_bool) == Some(true) {
            chat.insert(
                "thinking".to_string(),
                enabled_thinking_options(&route.provider, &route.model),
            );
        } else if let Some(thinking) = body.get("thinking").filter(|v| !v.is_null() && **v != Value::Bool(false)) {
            chat.insert("thinking".to_string(), thinking.clone());
        }
    }
    if let Some(tools) = responses_tools_to_chat_tools(body.get("tools")) {
        chat.insert("tools".to_string(), tools);
        copy_field(body, &mut chat, "tool_choice", "tool_choice");
    }
    Value::Object(chat)
}

fn responses_input_to_messages(value: Option<&Value>) -> Vec<Value> {
    match value {
        None => Vec::new(),
        Some(Value::String(text)) => vec![json!({"role": "user", "content": text})],
        Some(Value::Array(items)) => {
            let mut out = Vec::new();
            for item in items {
                match item {
                    Value::String(text) => out.push(json!({"role": "user", "content": text})),
                    Value::Object(map) => {
                        let item_type = map.get("type").and_then(Value::as_str);
                        if (item_type == Some("message") || item_type.is_none()) && map.contains_key("role") {
                            let mut role = map.get("role").and_then(Value::as_str).unwrap_or("user").to_string();
                            if role == "developer" {
                                role = "system".to_string();
                            }
                            out.push(json!({"role": role, "content": content_to_text(map.get("content").unwrap_or(&Value::Null))}));
                        } else if matches!(item_type, Some("input_text" | "text")) {
                            out.push(json!({"role": "user", "content": content_to_text(item)}));
                        } else if item_type == Some("function_call_output") {
                            out.push(json!({
                                "role": "tool",
                                "tool_call_id": map.get("call_id").cloned().unwrap_or(Value::Null),
                                "content": content_to_text(map.get("output").unwrap_or(&Value::Null))
                            }));
                        }
                    }
                    _ => {}
                }
            }
            out
        }
        Some(other) => vec![json!({"role": "user", "content": content_to_text(other)})],
    }
}

fn responses_tools_to_chat_tools(value: Option<&Value>) -> Option<Value> {
    let Value::Array(tools) = value? else {
        return None;
    };
    let converted: Vec<Value> = tools
        .iter()
        .filter_map(|tool| {
            let map = tool.as_object()?;
            if map.get("type").and_then(Value::as_str) != Some("function") {
                return None;
            }
            if map.contains_key("function") {
                return Some(tool.clone());
            }
            let name = map.get("name")?.clone();
            Some(json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": map.get("description").cloned().unwrap_or(Value::String(String::new())),
                    "parameters": map.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object"}))
                }
            }))
        })
        .collect();
    if converted.is_empty() {
        None
    } else {
        Some(Value::Array(converted))
    }
}

fn stream_openai_chat(upstream: reqwest::Response, model: String, as_responses: bool) -> Response<RespBody> {
    let (tx, rx) = mpsc::channel::<StreamResult>(32);
    tokio::spawn(async move {
        let result = if as_responses {
            stream_openai_chat_as_responses(upstream, model, tx.clone()).await
        } else {
            stream_openai_chat_passthrough(upstream, tx.clone()).await
        };
        if let Err(err) = result {
            let _ = tx.send(Ok(Frame::data(Bytes::from(sse_data(&json!({
                "type": "error",
                "error": {"message": err.to_string()}
            })))))).await;
        }
    });

    let stream = MpscBodyStream { rx };
    let body = StreamBody::new(stream).boxed();
    let mut response = Response::new(body);
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
        .headers_mut()
        .insert(CONNECTION, HeaderValue::from_static("keep-alive"));
    response
}

async fn stream_openai_chat_passthrough(
    upstream: reqwest::Response,
    tx: mpsc::Sender<StreamResult>,
) -> AppResult<()> {
    for line in collect_sse_data_lines(upstream).await? {
        let frame = if line == "[DONE]" {
            Bytes::from_static(b"data: [DONE]\n\n")
        } else {
            Bytes::from(format!("data: {line}\n\n"))
        };
        if tx.send(Ok(Frame::data(frame))).await.is_err() {
            break;
        }
        if line == "[DONE]" {
            break;
        }
    }
    Ok(())
}

async fn stream_openai_chat_as_responses(
    upstream: reqwest::Response,
    model: String,
    tx: mpsc::Sender<StreamResult>,
) -> AppResult<()> {
    let mut state = ResponsesStreamState::new(model);
    state.start(&tx).await?;
    for line in collect_sse_data_lines(upstream).await? {
        if line == "[DONE]" {
            break;
        }
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        state.write_chat_delta(&tx, &event).await?;
    }
    state.finish(&tx).await
}

async fn collect_sse_data_lines(mut upstream: reqwest::Response) -> AppResult<Vec<String>> {
    let mut lines = Vec::new();
    let mut buffer = String::new();
    while let Some(chunk) = upstream.chunk().await? {
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buffer.find('\n') {
            let raw: String = buffer.drain(..=pos).collect();
            let line = raw.trim();
            if let Some(rest) = line.strip_prefix("data:") {
                lines.push(rest.trim().to_string());
            }
        }
    }
    let tail = buffer.trim();
    if let Some(rest) = tail.strip_prefix("data:") {
        lines.push(rest.trim().to_string());
    }
    Ok(lines)
}

impl ResponsesStreamState {
    fn new(model: String) -> Self {
        let now = now_millis();
        Self {
            response_id: format!("resp_{now}"),
            message_item_id: format!("msg_{now}"),
            model,
            message_index: None,
            message_text: String::new(),
            message_opened: false,
            message_closed: false,
            tool_calls: HashMap::new(),
            reasoning: None,
            next_output_index: 0,
        }
    }

    async fn start(&self, tx: &mpsc::Sender<StreamResult>) -> AppResult<()> {
        write_sse(tx, &json!({"type": "response.created", "response": self.response("in_progress", false)})).await
    }

    async fn finish(&mut self, tx: &mpsc::Sender<StreamResult>) -> AppResult<()> {
        if self.message_opened && !self.message_closed {
            self.close_message(tx).await?;
        }
        let mut tool_keys: Vec<usize> = self.tool_calls.keys().copied().collect();
        tool_keys.sort_by_key(|key| self.tool_calls.get(key).map(|s| s.output_index).unwrap_or_default());
        for key in tool_keys {
            let should_close = self.tool_calls.get(&key).map(|s| !s.closed).unwrap_or(false);
            if should_close {
                self.close_tool(tx, key).await?;
            }
        }
        if self.reasoning.as_ref().map(|s| !s.closed).unwrap_or(false) {
            self.close_reasoning(tx).await?;
        }
        write_sse(tx, &json!({"type": "response.completed", "response": self.response("completed", true)})).await?;
        tx.send(Ok(Frame::data(Bytes::from_static(b"data: [DONE]\n\n"))))
            .await
            .map_err(|_| AppError::msg("downstream SSE client disconnected"))?;
        Ok(())
    }

    async fn write_chat_delta(&mut self, tx: &mpsc::Sender<StreamResult>, chunk: &Value) -> AppResult<()> {
        let delta = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("delta"))
            .unwrap_or(&Value::Null);
        if let Some(reasoning) = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            self.reasoning_delta(tx, reasoning).await?;
        }
        for reasoning in minimax_reasoning_detail_deltas(delta.get("reasoning_details")) {
            self.reasoning_delta(tx, &reasoning).await?;
        }
        if let Some(content) = delta.get("content").and_then(Value::as_str).filter(|value| !value.is_empty()) {
            self.text_delta(tx, content).await?;
        }
        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                self.tool_delta(tx, call).await?;
            }
        }
        Ok(())
    }

    async fn reasoning_delta(&mut self, tx: &mpsc::Sender<StreamResult>, text: &str) -> AppResult<()> {
        if self.reasoning.is_none() {
            let output_index = self.next_output_index;
            self.next_output_index += 1;
            let id = format!("rs_{}_{}", now_millis(), output_index);
            self.reasoning = Some(ReasoningState {
                id: id.clone(),
                text: String::new(),
                output_index,
                closed: false,
            });
            write_sse(
                tx,
                &json!({
                    "type": "response.output_item.added",
                    "output_index": output_index,
                    "item": {
                        "id": id,
                        "type": "reasoning",
                        "status": "in_progress",
                        "summary": [],
                        "encrypted_content": Value::Null
                    }
                }),
            )
            .await?;
        }
        let state = self.reasoning.as_mut().expect("reasoning opened");
        state.text.push_str(text);
        write_sse(
            tx,
            &json!({
                "type": "response.reasoning_summary_text.delta",
                "item_id": state.id,
                "output_index": state.output_index,
                "summary_index": 0,
                "delta": text
            }),
        )
        .await
    }

    async fn open_message(&mut self, tx: &mpsc::Sender<StreamResult>) -> AppResult<()> {
        self.message_index = Some(self.next_output_index);
        self.next_output_index += 1;
        self.message_opened = true;
        let index = self.message_index.unwrap_or_default();
        write_sse(
            tx,
            &json!({
                "type": "response.output_item.added",
                "output_index": index,
                "item": {
                    "id": self.message_item_id,
                    "type": "message",
                    "status": "in_progress",
                    "role": "assistant",
                    "content": []
                }
            }),
        )
        .await?;
        write_sse(
            tx,
            &json!({
                "type": "response.content_part.added",
                "item_id": self.message_item_id,
                "output_index": index,
                "content_index": 0,
                "part": {"type": "output_text", "text": "", "annotations": []}
            }),
        )
        .await
    }

    async fn text_delta(&mut self, tx: &mpsc::Sender<StreamResult>, text: &str) -> AppResult<()> {
        if !self.message_opened {
            self.open_message(tx).await?;
        }
        self.message_text.push_str(text);
        write_sse(
            tx,
            &json!({
                "type": "response.output_text.delta",
                "item_id": self.message_item_id,
                "output_index": self.message_index.unwrap_or_default(),
                "content_index": 0,
                "delta": text
            }),
        )
        .await
    }

    async fn close_message(&mut self, tx: &mpsc::Sender<StreamResult>) -> AppResult<()> {
        if !self.message_opened || self.message_closed {
            return Ok(());
        }
        self.message_closed = true;
        let output_index = self.message_index.unwrap_or_default();
        write_sse(
            tx,
            &json!({
                "type": "response.output_text.done",
                "item_id": self.message_item_id,
                "output_index": output_index,
                "content_index": 0,
                "text": self.message_text
            }),
        )
        .await?;
        write_sse(
            tx,
            &json!({
                "type": "response.content_part.done",
                "item_id": self.message_item_id,
                "output_index": output_index,
                "content_index": 0,
                "part": {"type": "output_text", "text": self.message_text, "annotations": []}
            }),
        )
        .await?;
        write_sse(
            tx,
            &json!({
                "type": "response.output_item.done",
                "output_index": output_index,
                "item": self.message_item("completed")
            }),
        )
        .await
    }

    async fn tool_delta(&mut self, tx: &mpsc::Sender<StreamResult>, call: &Value) -> AppResult<()> {
        let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        let fn_obj = call.get("function").unwrap_or(&Value::Null);
        if !self.tool_calls.contains_key(&index) {
            if self.message_opened && !self.message_closed {
                self.close_message(tx).await?;
            }
            let call_id = call
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("call_{index}"));
            let output_index = self.next_output_index;
            self.next_output_index += 1;
            let name = fn_obj.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            self.tool_calls.insert(
                index,
                ToolCallState {
                    id: call_id.clone(),
                    call_id: call_id.clone(),
                    name: name.clone(),
                    arguments: String::new(),
                    output_index,
                    closed: false,
                },
            );
            write_sse(
                tx,
                &json!({
                    "type": "response.output_item.added",
                    "output_index": output_index,
                    "item": {
                        "id": call_id,
                        "type": "function_call",
                        "status": "in_progress",
                        "call_id": self.tool_calls.get(&index).map(|s| s.call_id.clone()).unwrap_or_default(),
                        "name": name,
                        "arguments": ""
                    }
                }),
            )
            .await?;
        } else if let Some(name) = fn_obj.get("name").and_then(Value::as_str).filter(|value| !value.is_empty()) {
            if let Some(state) = self.tool_calls.get_mut(&index) {
                state.name.push_str(name);
            }
        }
        let arg_delta = fn_obj.get("arguments").and_then(Value::as_str).unwrap_or("");
        if !arg_delta.is_empty() {
            let state = self.tool_calls.get_mut(&index).expect("tool call opened");
            state.arguments.push_str(arg_delta);
            write_sse(
                tx,
                &json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": state.id,
                    "output_index": state.output_index,
                    "delta": arg_delta
                }),
            )
            .await?;
        }
        Ok(())
    }

    async fn close_tool(&mut self, tx: &mpsc::Sender<StreamResult>, key: usize) -> AppResult<()> {
        let Some(state) = self.tool_calls.get_mut(&key) else {
            return Ok(());
        };
        state.closed = true;
        let done_item = json!({
            "id": state.id,
            "type": "function_call",
            "status": "completed",
            "call_id": state.call_id,
            "name": state.name,
            "arguments": state.arguments
        });
        write_sse(
            tx,
            &json!({
                "type": "response.function_call_arguments.done",
                "item_id": state.id,
                "output_index": state.output_index,
                "arguments": state.arguments
            }),
        )
        .await?;
        write_sse(
            tx,
            &json!({
                "type": "response.output_item.done",
                "output_index": state.output_index,
                "item": done_item
            }),
        )
        .await
    }

    async fn close_reasoning(&mut self, tx: &mpsc::Sender<StreamResult>) -> AppResult<()> {
        let Some(state) = self.reasoning.as_mut() else {
            return Ok(());
        };
        state.closed = true;
        write_sse(
            tx,
            &json!({
                "type": "response.reasoning_summary_text.done",
                "item_id": state.id,
                "output_index": state.output_index,
                "summary_index": 0,
                "text": state.text
            }),
        )
        .await?;
        let item = reasoning_item(state);
        write_sse(
            tx,
            &json!({
                "type": "response.output_item.done",
                "output_index": state.output_index,
                "item": item
            }),
        )
        .await
    }

    fn message_item(&self, status: &str) -> Value {
        let content = if self.message_text.is_empty() {
            Vec::new()
        } else {
            vec![json!({"type": "output_text", "text": self.message_text, "annotations": []})]
        };
        json!({
            "id": self.message_item_id,
            "type": "message",
            "status": status,
            "role": "assistant",
            "content": content
        })
    }

    fn response(&self, status: &str, final_response: bool) -> Value {
        let mut output_items: Vec<(usize, Value)> = Vec::new();
        if final_response {
            if let Some(reasoning) = &self.reasoning {
                output_items.push((reasoning.output_index, reasoning_item(reasoning)));
            }
            if self.message_opened && !self.message_text.is_empty() {
                output_items.push((self.message_index.unwrap_or_default(), self.message_item("completed")));
            }
            for state in self.tool_calls.values() {
                output_items.push((
                    state.output_index,
                    json!({
                        "id": state.id,
                        "type": "function_call",
                        "status": "completed",
                        "call_id": state.call_id,
                        "name": state.name,
                        "arguments": state.arguments
                    }),
                ));
            }
            output_items.sort_by_key(|(index, _)| *index);
        }
        json!({
            "id": self.response_id,
            "object": "response",
            "created_at": now_secs(),
            "status": status,
            "model": self.model,
            "output": output_items.into_iter().map(|(_, item)| item).collect::<Vec<_>>()
        })
    }
}

fn minimax_reasoning_detail_deltas(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            item.get("text")
                .or_else(|| item.get("reasoning_content"))
                .or_else(|| item.get("content"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

fn reasoning_item(state: &ReasoningState) -> Value {
    json!({
        "id": state.id,
        "type": "reasoning",
        "status": "completed",
        "summary": if state.text.is_empty() {
            Vec::<Value>::new()
        } else {
            vec![json!({"type": "summary_text", "text": state.text})]
        },
        "encrypted_content": encode_thinking_payload(&state.text)
    })
}

fn encode_thinking_payload(text: &str) -> Value {
    let payload = json!({"type": "thinking", "thinking": text, "signature": ""});
    let raw = payload.to_string();
    Value::String(format!(
        "anthropic-thinking-v1:{}",
        base64::engine::general_purpose::URL_SAFE.encode(raw.as_bytes())
    ))
}

async fn write_sse(tx: &mpsc::Sender<StreamResult>, payload: &Value) -> AppResult<()> {
    tx.send(Ok(Frame::data(Bytes::from(sse_data(payload)))))
        .await
        .map_err(|_| AppError::msg("downstream SSE client disconnected"))
}

fn sse_data(payload: &Value) -> String {
    format!("data: {}\n\n", payload)
}

fn chat_completion_to_response(payload: Value, requested_model: &str) -> Value {
    let message = payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut output = Vec::new();
    let reasoning = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| minimax_reasoning(&message));
    if let Some(text) = reasoning.filter(|v| !v.is_empty()) {
        output.push(json!({
            "id": "rs_0",
            "type": "reasoning",
            "status": "completed",
            "summary": [{"type": "summary_text", "text": text}],
            "encrypted_content": encode_thinking_payload(&text)
        }));
    }
    let text = message
        .get("content")
        .and_then(Value::as_str)
        .map(strip_think)
        .unwrap_or_default();
    if !text.is_empty() {
        output.push(json!({
            "id": "msg_0",
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{"type": "output_text", "text": text, "annotations": []}]
        }));
    }
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            let fn_obj = call.get("function").unwrap_or(&Value::Null);
            let id = call.get("id").and_then(Value::as_str).unwrap_or("call_0");
            output.push(json!({
                "id": id,
                "type": "function_call",
                "status": "completed",
                "call_id": id,
                "name": fn_obj.get("name").and_then(Value::as_str).unwrap_or(""),
                "arguments": fn_obj.get("arguments").and_then(Value::as_str).unwrap_or("")
            }));
        }
    }
    json!({
        "id": payload.get("id").cloned().unwrap_or_else(|| Value::String("resp_chat".to_string())),
        "object": "response",
        "created_at": payload.get("created").cloned().unwrap_or(Value::Number(0.into())),
        "status": "completed",
        "model": requested_model,
        "output": output,
        "usage": payload.get("usage").cloned().unwrap_or(Value::Null)
    })
}

fn content_to_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| match part {
                Value::String(text) => Some(text.clone()),
                Value::Object(map) => map
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| map.get("content").map(content_to_text)),
                _ => None,
            })
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => map
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| value.to_string()),
        other => other.to_string(),
    }
}

fn copy_field(src: &Value, dst: &mut Map<String, Value>, from: &str, to: &str) {
    if let Some(value) = src.get(from) {
        dst.insert(to.to_string(), value.clone());
    }
}

fn provider_accepts_thinking(provider: &str, model: &str) -> bool {
    provider == "deepseek"
        || provider == "generic-chat-completion-api"
        || (provider == "moonshot" && model.starts_with("kimi-"))
}

fn enabled_thinking_options(provider: &str, model: &str) -> Value {
    if provider == "moonshot" && model.starts_with("kimi-") {
        json!({"type": "enabled", "keep": "all"})
    } else {
        json!({"type": "enabled"})
    }
}

fn is_openai_chat(provider: &str) -> bool {
    matches!(
        provider,
        "openai"
            | "generic-chat-completion-api"
            | "deepseek"
            | "mimo"
            | "minimax"
            | "moonshot"
            | "dashscope"
            | "volcengine"
    )
}

fn join_url(base_url: &str, endpoint: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1") {
        format!("{base}{endpoint}")
    } else if endpoint == "/messages" {
        format!("{base}/v1/messages")
    } else {
        format!("{base}/v1{endpoint}")
    }
}

fn minimax_reasoning(message: &Value) -> Option<String> {
    let chunks: Vec<String> = message
        .get("reasoning_details")?
        .as_array()?
        .iter()
        .filter_map(|item| {
            item.get("text")
                .or_else(|| item.get("reasoning_content"))
                .or_else(|| item.get("content"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    if chunks.is_empty() {
        None
    } else {
        Some(chunks.join("\n"))
    }
}

fn strip_think(text: &str) -> String {
    let mut output = String::new();
    let mut rest = text;
    while let Some(start) = rest.to_lowercase().find("<think>") {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + "<think>".len()..];
        if let Some(end) = after_start.to_lowercase().find("</think>") {
            rest = &after_start[end + "</think>".len()..];
        } else {
            rest = "";
        }
    }
    output.push_str(rest);
    output
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in value.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "model".to_string()
    } else {
        slug
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn json_response(status: StatusCode, value: Value) -> Response<RespBody> {
    let mut response = Response::new(full_body(Bytes::from(value.to_string())));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

fn text_response(status: StatusCode, text: impl Into<String>) -> Response<RespBody> {
    let mut response = Response::new(full_body(Bytes::from(text.into())));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
    response
}

fn error_response(err: AppError) -> Response<RespBody> {
    text_response(StatusCode::BAD_GATEWAY, err.to_string())
}

fn full_body(bytes: Bytes) -> RespBody {
    Full::new(bytes).map_err(|never| match never {}).boxed()
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_to_chat_drops_thinking_for_mimo() {
        let route = RouteModel {
            slug: "mimo-v2-5-pro".to_string(),
            model: "mimo-v2.5-pro".to_string(),
            provider: "mimo".to_string(),
            base_url: "https://token-plan-cn.xiaomimimo.com/v1".to_string(),
            api_key: String::new(),
            extra_headers: HashMap::new(),
        };
        let out = responses_to_chat(
            &json!({"input": "hi", "thinking": true, "stream": true}),
            &route,
        );
        assert!(out.get("thinking").is_none());
        assert_eq!(out["stream"], Value::Bool(true));
    }

    #[test]
    fn responses_to_chat_keeps_kimi_thinking_all() {
        let route = RouteModel {
            slug: "kimi-k2-6".to_string(),
            model: "kimi-k2.6".to_string(),
            provider: "moonshot".to_string(),
            base_url: "https://api.moonshot.cn/v1".to_string(),
            api_key: String::new(),
            extra_headers: HashMap::new(),
        };
        let out = responses_to_chat(&json!({"input": "hi", "thinking": true}), &route);
        assert_eq!(out["thinking"], json!({"type": "enabled", "keep": "all"}));
    }

    #[test]
    fn chat_completion_to_response_preserves_minimax_reasoning() {
        let out = chat_completion_to_response(
            json!({
                "id": "chatcmpl_1",
                "choices": [{
                    "message": {
                        "content": "Answer",
                        "reasoning_details": [
                            {"text": "First"},
                            {"text": "Second"}
                        ]
                    }
                }]
            }),
            "minimax-m2",
        );
        assert_eq!(out["output"][0]["type"], "reasoning");
        assert_eq!(out["output"][0]["summary"][0]["text"], "First\nSecond");
        assert!(out["output"][0]["encrypted_content"]
            .as_str()
            .unwrap()
            .starts_with("anthropic-thinking-v1:"));
        assert_eq!(out["output"][1]["content"][0]["text"], "Answer");
    }

    #[test]
    fn response_stream_state_emits_reasoning_text_and_tool_items() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (tx, mut rx) = mpsc::channel::<StreamResult>(32);
            let mut state = ResponsesStreamState::new("deepseek-reasoner".to_string());
            state.start(&tx).await.unwrap();
            state
                .write_chat_delta(
                    &tx,
                    &json!({"choices":[{"delta":{"reasoning_content":"think ","content":"hi"}}]}),
                )
                .await
                .unwrap();
            state
                .write_chat_delta(
                    &tx,
                    &json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"run","arguments":"{\"cmd\""}}]}}]}),
                )
                .await
                .unwrap();
            state.finish(&tx).await.unwrap();
            drop(tx);

            let mut raw = String::new();
            while let Some(Ok(frame)) = rx.recv().await {
                if let Some(bytes) = frame.data_ref() {
                    raw.push_str(&String::from_utf8_lossy(bytes));
                }
            }
            assert!(raw.contains("\"response.reasoning_summary_text.delta\""));
            assert!(raw.contains("\"response.output_text.delta\""));
            assert!(raw.contains("\"response.function_call_arguments.delta\""));
            assert!(raw.contains("\"response.completed\""));
            assert!(raw.contains("data: [DONE]"));
        });
    }

    #[test]
    fn join_url_appends_chat_completions_like_python() {
        assert_eq!(
            join_url("https://token-plan-cn.xiaomimimo.com/v1", "/chat/completions"),
            "https://token-plan-cn.xiaomimimo.com/v1/chat/completions"
        );
        assert_eq!(
            join_url("https://api.example.com", "/chat/completions"),
            "https://api.example.com/v1/chat/completions"
        );
    }
}
