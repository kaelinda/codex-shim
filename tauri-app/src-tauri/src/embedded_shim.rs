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
use tokio::fs;
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
    max_output_tokens: Option<i64>,
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
        return post_chatgpt_passthrough(ctx, body).await;
    }
    let route = find_route(&ctx.settings_path, requested).await?;
    if is_openai_chat(&route.provider) {
        let forwarded = responses_to_chat(&body, &route);
        return post_openai_chat(ctx, &route, forwarded, true).await;
    }
    if route.provider == "anthropic" {
        let forwarded = responses_to_anthropic(&body, &route);
        return post_anthropic(ctx, &route, forwarded, true).await;
    }
    Ok(text_response(
        StatusCode::BAD_GATEWAY,
        format!("Unsupported model provider: {}", route.provider),
    ))
}

async fn chat_completions_response(req: Request<Incoming>, ctx: &ServerContext) -> AppResult<Response<RespBody>> {
    let mut body = read_json(req).await?;
    let requested = body.get("model").and_then(Value::as_str).unwrap_or_default();
    if requested == "gpt-5.5" || requested.starts_with("openai-gpt-5-5") {
        if let Value::Object(map) = &mut body {
            map.insert("model".to_string(), Value::String("gpt-5.5".to_string()));
        }
        return post_chatgpt_passthrough(ctx, chat_to_responses_request(&body, "gpt-5.5", None)).await;
    }
    let route = find_route(&ctx.settings_path, requested).await?;
    if is_openai_chat(&route.provider) {
        if let Value::Object(map) = &mut body {
            map.insert("model".to_string(), Value::String(route.model.clone()));
        }
        return post_openai_chat(ctx, &route, body, false).await;
    }
    if route.provider == "anthropic" {
        let forwarded = chat_to_anthropic(&body, &route);
        return post_anthropic(ctx, &route, forwarded, false).await;
    }
    if !is_openai_chat(&route.provider) {
        return Ok(text_response(
            StatusCode::BAD_GATEWAY,
            format!("Unsupported model provider: {}", route.provider),
        ));
    }
    unreachable!()
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

async fn post_anthropic(
    ctx: &ServerContext,
    route: &RouteModel,
    body: Value,
    as_responses: bool,
) -> AppResult<Response<RespBody>> {
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let url = join_url(&route.base_url, "/messages");
    let mut request = ctx
        .client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&body);
    if !route.api_key.is_empty() {
        request = request.header("x-api-key", &route.api_key);
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
        return Ok(stream_anthropic(response, route.slug.clone(), as_responses));
    }
    let payload: Value = response.json().await?;
    if as_responses {
        Ok(json_response(
            StatusCode::OK,
            anthropic_to_response(payload, &route.slug),
        ))
    } else {
        Ok(json_response(
            StatusCode::OK,
            anthropic_to_chat_response(payload, &route.slug),
        ))
    }
}

async fn post_chatgpt_passthrough(ctx: &ServerContext, body: Value) -> AppResult<Response<RespBody>> {
    let auth = read_chatgpt_auth().await?;
    let mut forwarded = body;
    if let Value::Object(map) = &mut forwarded {
        map.insert("model".to_string(), Value::String("gpt-5.5".to_string()));
    }
    let stream = forwarded.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let mut request = ctx
        .client
        .post("https://chatgpt.com/backend-api/codex/responses")
        .bearer_auth(&auth.access_token)
        .header(CONTENT_TYPE, "application/json")
        .header("Accept", if stream { "text/event-stream" } else { "application/json" })
        .header("OpenAI-Beta", "responses=2026-02-06")
        .header("originator", "codex_cli_rs")
        .json(&forwarded);
    if let Some(account_id) = auth.account_id.filter(|value| !value.is_empty()) {
        request = request.header("chatgpt-account-id", account_id);
    }
    let response = request.send().await?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await?;
        return Err(AppError::msg(format!("ChatGPT passthrough returned {status}: {text}")));
    }
    if stream {
        Ok(stream_raw_sse(response))
    } else {
        let payload: Value = response.json().await?;
        Ok(json_response(StatusCode::OK, payload))
    }
}

struct ChatGptAuth {
    access_token: String,
    account_id: Option<String>,
}

async fn read_chatgpt_auth() -> AppResult<ChatGptAuth> {
    let path = codex_auth_path();
    let text = fs::read_to_string(&path)
        .await
        .map_err(|_| AppError::msg("~/.codex/auth.json not found"))?;
    let parsed: Value = serde_json::from_str(&text)?;
    let tokens = parsed.get("tokens").and_then(Value::as_object);
    let access_token = tokens
        .and_then(|map| map.get("access_token"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::msg("auth.json has no access_token"))?
        .to_string();
    let account_id = parsed
        .get("account_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            tokens
                .and_then(|map| map.get("account_id"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    Ok(ChatGptAuth {
        access_token,
        account_id,
    })
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
            max_output_tokens: row.max_output_tokens,
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
    let mut pending_reasoning = String::new();
    let mut had_reasoning = false;
    for mut message in responses_input_to_messages(body.get("input")) {
        if message.get("_reasoning_only").and_then(Value::as_bool) == Some(true) {
            if let Some(text) = message.get("reasoning_content").and_then(Value::as_str) {
                if !text.is_empty() {
                    if !pending_reasoning.is_empty() {
                        pending_reasoning.push('\n');
                    }
                    pending_reasoning.push_str(text);
                    had_reasoning = true;
                }
            }
            continue;
        }
        if message.get("role").and_then(Value::as_str) == Some("assistant") && !pending_reasoning.is_empty() {
            if let Some(map) = message.as_object_mut() {
                map.insert(
                    "reasoning_content".to_string(),
                    Value::String(pending_reasoning.clone()),
                );
            }
            pending_reasoning.clear();
        }
        messages.push(message);
    }
    if !pending_reasoning.is_empty() {
        messages.push(json!({"role": "assistant", "content": "", "reasoning_content": pending_reasoning}));
    }
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
        } else if had_reasoning {
            chat.insert(
                "thinking".to_string(),
                enabled_thinking_options(&route.provider, &route.model),
            );
        }
    }
    if let Some(tools) = responses_tools_to_chat_tools(body.get("tools")) {
        chat.insert("tools".to_string(), tools);
        copy_field(body, &mut chat, "tool_choice", "tool_choice");
    }
    Value::Object(chat)
}

fn responses_to_anthropic(body: &Value, route: &RouteModel) -> Value {
    let mut system_parts = Vec::new();
    if let Some(instructions) = body.get("instructions") {
        let text = content_to_text(instructions);
        if !text.is_empty() {
            system_parts.push(text);
        }
    }

    let mut messages: Vec<Value> = Vec::new();
    let mut pending_thinking: Vec<Value> = Vec::new();
    for chat_msg in responses_input_to_messages(body.get("input")) {
        let role = chat_msg.get("role").and_then(Value::as_str).unwrap_or("user");
        if chat_msg.get("_reasoning_only").and_then(Value::as_bool) == Some(true) {
            if let Some(decoded) = chat_msg
                .get("encrypted_content")
                .and_then(Value::as_str)
                .and_then(decode_thinking_payload)
            {
                pending_thinking.push(decoded);
            } else if let Some(text) = chat_msg.get("reasoning_content").and_then(Value::as_str).filter(|v| !v.is_empty()) {
                pending_thinking.push(json!({"type": "thinking", "thinking": text, "signature": ""}));
            }
            continue;
        }
        if role == "system" || role == "developer" {
            let text = content_to_text(chat_msg.get("content").unwrap_or(&Value::Null));
            if !text.is_empty() {
                system_parts.push(text);
            }
            continue;
        }
        if role == "assistant" {
            let mut blocks = Vec::new();
            blocks.append(&mut pending_thinking);
            let text = content_to_text(chat_msg.get("content").unwrap_or(&Value::Null));
            if !text.is_empty() {
                blocks.push(json!({"type": "text", "text": text}));
            }
            if let Some(calls) = chat_msg.get("tool_calls").and_then(Value::as_array) {
                for call in calls {
                    let fn_obj = call.get("function").unwrap_or(&Value::Null);
                    let args_raw = fn_obj.get("arguments").and_then(Value::as_str).unwrap_or("");
                    let input = serde_json::from_str::<Value>(args_raw).unwrap_or_else(|_| json!({"_raw": args_raw}));
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": call.get("id").and_then(Value::as_str).unwrap_or("call_0"),
                        "name": fn_obj.get("name").and_then(Value::as_str).unwrap_or(""),
                        "input": input
                    }));
                }
            }
            if !blocks.is_empty() {
                append_anthropic_message(&mut messages, "assistant", Value::Array(blocks));
            }
            continue;
        }
        if role == "tool" {
            pending_thinking.clear();
            append_anthropic_message(
                &mut messages,
                "user",
                json!([{
                    "type": "tool_result",
                    "tool_use_id": chat_msg.get("tool_call_id").and_then(Value::as_str).unwrap_or("call_0"),
                    "content": content_to_text(chat_msg.get("content").unwrap_or(&Value::Null))
                }]),
            );
            continue;
        }
        pending_thinking.clear();
        append_anthropic_message(
            &mut messages,
            role,
            Value::String(content_to_text(chat_msg.get("content").unwrap_or(&Value::Null))),
        );
    }
    if !pending_thinking.is_empty() {
        append_anthropic_message(&mut messages, "assistant", Value::Array(pending_thinking));
    }

    let mut out = Map::new();
    out.insert("model".to_string(), Value::String(route.model.clone()));
    out.insert(
        "messages".to_string(),
        if messages.is_empty() {
            json!([{"role": "user", "content": ""}])
        } else {
            Value::Array(messages)
        },
    );
    out.insert(
        "max_tokens".to_string(),
        body.get("max_output_tokens")
            .or_else(|| body.get("max_tokens"))
            .cloned()
            .or_else(|| route.max_output_tokens.map(|v| Value::Number(v.into())))
            .unwrap_or_else(|| Value::Number(4096.into())),
    );
    out.insert(
        "stream".to_string(),
        Value::Bool(body.get("stream").and_then(Value::as_bool).unwrap_or(false)),
    );
    if !system_parts.is_empty() {
        out.insert("system".to_string(), Value::String(system_parts.join("\n\n")));
    }
    copy_field(body, &mut out, "temperature", "temperature");
    copy_field(body, &mut out, "top_p", "top_p");
    if let Some(tools) = responses_tools_to_anthropic_tools(body.get("tools")) {
        out.insert("tools".to_string(), tools);
    }
    Value::Object(out)
}

fn chat_to_responses_request(body: &Value, upstream_model: &str, max_tokens: Option<i64>) -> Value {
    let mut out = Map::new();
    out.insert("model".to_string(), Value::String(upstream_model.to_string()));
    out.insert(
        "input".to_string(),
        body.get("messages").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
    );
    out.insert(
        "stream".to_string(),
        Value::Bool(body.get("stream").and_then(Value::as_bool).unwrap_or(false)),
    );
    copy_field(body, &mut out, "temperature", "temperature");
    copy_field(body, &mut out, "top_p", "top_p");
    copy_field(body, &mut out, "max_tokens", "max_output_tokens");
    if max_tokens.is_some() && !out.contains_key("max_output_tokens") {
        out.insert("max_output_tokens".to_string(), Value::Number(max_tokens.unwrap().into()));
    }
    if let Some(tools) = body.get("tools") {
        out.insert("tools".to_string(), tools.clone());
    }
    Value::Object(out)
}

fn chat_to_anthropic(body: &Value, route: &RouteModel) -> Value {
    responses_to_anthropic(&chat_to_responses_request(body, &route.model, route.max_output_tokens), route)
}

fn responses_input_to_messages(value: Option<&Value>) -> Vec<Value> {
    match value {
        None => Vec::new(),
        Some(Value::String(text)) => vec![json!({"role": "user", "content": text})],
        Some(Value::Array(items)) => {
            let mut out = Vec::new();
            let mut pending_tool_calls: Vec<Value> = Vec::new();
            for item in items {
                match item {
                    Value::String(text) => {
                        flush_pending_tool_calls(&mut out, &mut pending_tool_calls);
                        out.push(json!({"role": "user", "content": text}));
                    }
                    Value::Object(map) => {
                        let item_type = map.get("type").and_then(Value::as_str);
                        if (item_type == Some("message") || item_type.is_none()) && map.contains_key("role") {
                            flush_pending_tool_calls(&mut out, &mut pending_tool_calls);
                            let mut role = map.get("role").and_then(Value::as_str).unwrap_or("user").to_string();
                            if role == "developer" {
                                role = "system".to_string();
                            }
                            out.push(json!({"role": role, "content": content_to_text(map.get("content").unwrap_or(&Value::Null))}));
                        } else if matches!(item_type, Some("input_text" | "text")) {
                            flush_pending_tool_calls(&mut out, &mut pending_tool_calls);
                            out.push(json!({"role": "user", "content": content_to_text(item)}));
                        } else if item_type == Some("function_call") {
                            let call_id = map
                                .get("call_id")
                                .or_else(|| map.get("id"))
                                .and_then(Value::as_str)
                                .unwrap_or("call_0");
                            pending_tool_calls.push(json!({
                                "id": call_id,
                                "type": "function",
                                "function": {
                                    "name": map.get("name").and_then(Value::as_str).unwrap_or(""),
                                    "arguments": map.get("arguments").and_then(Value::as_str).unwrap_or("")
                                }
                            }));
                        } else if item_type == Some("function_call_output") {
                            flush_pending_tool_calls(&mut out, &mut pending_tool_calls);
                            out.push(json!({
                                "role": "tool",
                                "tool_call_id": map.get("call_id").cloned().unwrap_or(Value::Null),
                                "content": content_to_text(map.get("output").unwrap_or(&Value::Null))
                            }));
                        } else if item_type == Some("reasoning") {
                            flush_pending_tool_calls(&mut out, &mut pending_tool_calls);
                            let mut reasoning = reasoning_text_from_item(item);
                            if let Some(decoded) = map
                                .get("encrypted_content")
                                .and_then(Value::as_str)
                                .and_then(decode_thinking_payload)
                            {
                                if let Some(text) = decoded.get("thinking").and_then(Value::as_str).filter(|v| !v.is_empty()) {
                                    reasoning = text.to_string();
                                }
                            }
                            if !reasoning.is_empty() {
                                let mut msg = json!({
                                    "role": "assistant",
                                    "_reasoning_only": true,
                                    "content": reasoning,
                                    "reasoning_content": reasoning
                                });
                                if let Some(encrypted) = map.get("encrypted_content") {
                                    msg.as_object_mut()
                                        .unwrap()
                                        .insert("encrypted_content".to_string(), encrypted.clone());
                                }
                                out.push(msg);
                            }
                        }
                    }
                    _ => {}
                }
            }
            flush_pending_tool_calls(&mut out, &mut pending_tool_calls);
            out
        }
        Some(other) => vec![json!({"role": "user", "content": content_to_text(other)})],
    }
}

fn flush_pending_tool_calls(out: &mut Vec<Value>, pending: &mut Vec<Value>) {
    if pending.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut().and_then(Value::as_object_mut) {
        if last.get("role").and_then(Value::as_str) == Some("assistant")
            && !last.contains_key("tool_calls")
            && last.get("_reasoning_only").and_then(Value::as_bool) != Some(true)
        {
            last.insert("tool_calls".to_string(), Value::Array(std::mem::take(pending)));
            return;
        }
    }
    out.push(json!({"role": "assistant", "content": Value::Null, "tool_calls": std::mem::take(pending)}));
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

fn responses_tools_to_anthropic_tools(value: Option<&Value>) -> Option<Value> {
    let tools = responses_tools_to_chat_tools(value)?;
    let converted: Vec<Value> = tools
        .as_array()?
        .iter()
        .filter_map(|tool| {
            let fn_obj = tool.get("function")?;
            let name = fn_obj.get("name")?.clone();
            Some(json!({
                "name": name,
                "description": fn_obj.get("description").cloned().unwrap_or(Value::String(String::new())),
                "input_schema": fn_obj.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object", "properties": {}}))
            }))
        })
        .collect();
    if converted.is_empty() {
        None
    } else {
        Some(Value::Array(converted))
    }
}

fn append_anthropic_message(messages: &mut Vec<Value>, role: &str, content: Value) {
    if let Some(last) = messages.last_mut().and_then(Value::as_object_mut) {
        if last.get("role").and_then(Value::as_str) == Some(role) {
            if let (Some(existing), Value::Array(mut incoming)) = (
                last.get_mut("content").and_then(Value::as_array_mut),
                content.clone(),
            ) {
                existing.append(&mut incoming);
                return;
            }
        }
    }
    messages.push(json!({"role": role, "content": content}));
}

fn reasoning_text_from_item(item: &Value) -> String {
    item.get("summary")
        .and_then(Value::as_array)
        .map(|summary| {
            summary
                .iter()
                .filter_map(|part| {
                    part.as_str()
                        .map(str::to_string)
                        .or_else(|| part.get("text").and_then(Value::as_str).map(str::to_string))
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
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

fn stream_anthropic(upstream: reqwest::Response, model: String, as_responses: bool) -> Response<RespBody> {
    let (tx, rx) = mpsc::channel::<StreamResult>(32);
    tokio::spawn(async move {
        let result = if as_responses {
            stream_anthropic_as_responses(upstream, model, tx.clone()).await
        } else {
            stream_anthropic_as_chat(upstream, model, tx.clone()).await
        };
        if let Err(err) = result {
            let _ = tx.send(Ok(Frame::data(Bytes::from(sse_data(&json!({
                "type": "error",
                "error": {"message": err.to_string()}
            })))))).await;
        }
    });
    sse_response_from_rx(rx)
}

fn stream_raw_sse(upstream: reqwest::Response) -> Response<RespBody> {
    let (tx, rx) = mpsc::channel::<StreamResult>(32);
    tokio::spawn(async move {
        let result = stream_raw_sse_body(upstream, tx.clone()).await;
        if let Err(err) = result {
            let _ = tx.send(Ok(Frame::data(Bytes::from(sse_data(&json!({
                "type": "error",
                "error": {"message": err.to_string()}
            })))))).await;
        }
    });
    sse_response_from_rx(rx)
}

fn sse_response_from_rx(rx: mpsc::Receiver<StreamResult>) -> Response<RespBody> {
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
    mut upstream: reqwest::Response,
    tx: mpsc::Sender<StreamResult>,
) -> AppResult<()> {
    let mut reader = SseLineReader::default();
    while let Some(chunk) = upstream.chunk().await? {
        for line in reader.push(&chunk) {
            let done = line == "[DONE]";
            let frame = if done {
                Bytes::from_static(b"data: [DONE]\n\n")
            } else {
                Bytes::from(format!("data: {line}\n\n"))
            };
            if tx.send(Ok(Frame::data(frame))).await.is_err() || done {
                return Ok(());
            }
        }
    }
    if let Some(line) = reader.finish() {
        let frame = if line == "[DONE]" {
            Bytes::from_static(b"data: [DONE]\n\n")
        } else {
            Bytes::from(format!("data: {line}\n\n"))
        };
        let _ = tx.send(Ok(Frame::data(frame))).await;
    }
    Ok(())
}

async fn stream_raw_sse_body(
    mut upstream: reqwest::Response,
    tx: mpsc::Sender<StreamResult>,
) -> AppResult<()> {
    while let Some(chunk) = upstream.chunk().await? {
        if tx.send(Ok(Frame::data(chunk))).await.is_err() {
            break;
        }
    }
    Ok(())
}

async fn stream_anthropic_as_chat(
    mut upstream: reqwest::Response,
    model: String,
    tx: mpsc::Sender<StreamResult>,
) -> AppResult<()> {
    let mut reader = SseLineReader::default();
    while let Some(chunk) = upstream.chunk().await? {
        for line in reader.push(&chunk) {
            if line == "[DONE]" {
                let _ = tx.send(Ok(Frame::data(Bytes::from_static(b"data: [DONE]\n\n")))).await;
                return Ok(());
            }
            let Ok(event) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            write_sse(&tx, &anthropic_stream_to_chat_chunk(&event, &model)).await?;
        }
    }
    if let Some(line) = reader.finish() {
        if let Ok(event) = serde_json::from_str::<Value>(&line) {
            write_sse(&tx, &anthropic_stream_to_chat_chunk(&event, &model)).await?;
        }
    }
    tx.send(Ok(Frame::data(Bytes::from_static(b"data: [DONE]\n\n"))))
        .await
        .ok();
    Ok(())
}

async fn stream_anthropic_as_responses(
    mut upstream: reqwest::Response,
    model: String,
    tx: mpsc::Sender<StreamResult>,
) -> AppResult<()> {
    let mut state = ResponsesStreamState::new(model);
    state.start(&tx).await?;
    let mut reader = SseLineReader::default();
    while let Some(chunk) = upstream.chunk().await? {
        for line in reader.push(&chunk) {
            if line == "[DONE]" {
                state.finish(&tx).await?;
                return Ok(());
            }
            let Ok(event) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            state.write_anthropic_delta(&tx, &event).await?;
        }
    }
    if let Some(line) = reader.finish() {
        if let Ok(event) = serde_json::from_str::<Value>(&line) {
            state.write_anthropic_delta(&tx, &event).await?;
        }
    }
    state.finish(&tx).await
}

async fn stream_openai_chat_as_responses(
    mut upstream: reqwest::Response,
    model: String,
    tx: mpsc::Sender<StreamResult>,
) -> AppResult<()> {
    let mut state = ResponsesStreamState::new(model);
    state.start(&tx).await?;
    let mut reader = SseLineReader::default();
    while let Some(chunk) = upstream.chunk().await? {
        for line in reader.push(&chunk) {
            if line == "[DONE]" {
                state.finish(&tx).await?;
                return Ok(());
            }
            let Ok(event) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            state.write_chat_delta(&tx, &event).await?;
        }
    }
    if let Some(line) = reader.finish() {
        if let Ok(event) = serde_json::from_str::<Value>(&line) {
            state.write_chat_delta(&tx, &event).await?;
        }
    }
    state.finish(&tx).await
}

#[derive(Default)]
struct SseLineReader {
    buffer: String,
}

impl SseLineReader {
    fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buffer.push_str(&String::from_utf8_lossy(chunk));
        let mut lines = Vec::new();
        while let Some(pos) = self.buffer.find('\n') {
            let raw: String = self.buffer.drain(..=pos).collect();
            if let Some(line) = parse_sse_data_line(&raw) {
                lines.push(line);
            }
        }
        lines
    }

    fn finish(&mut self) -> Option<String> {
        let tail = std::mem::take(&mut self.buffer);
        parse_sse_data_line(&tail)
    }
}

fn parse_sse_data_line(raw: &str) -> Option<String> {
    raw.trim()
        .strip_prefix("data:")
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
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

    async fn write_anthropic_delta(&mut self, tx: &mpsc::Sender<StreamResult>, event: &Value) -> AppResult<()> {
        match event.get("type").and_then(Value::as_str).unwrap_or_default() {
            "content_block_start" => {
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let block = event.get("content_block").unwrap_or(&Value::Null);
                match block.get("type").and_then(Value::as_str).unwrap_or_default() {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(Value::as_str).filter(|v| !v.is_empty()) {
                            self.text_delta(tx, text).await?;
                        }
                    }
                    "tool_use" => {
                        let call_id = block
                            .get("id")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("call_{index}"));
                        let name = block.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                        self.open_tool(tx, index, call_id, name).await?;
                    }
                    "thinking" | "redacted_thinking" => {
                        if let Some(text) = block.get("thinking").and_then(Value::as_str).filter(|v| !v.is_empty()) {
                            self.reasoning_delta(tx, text).await?;
                        } else {
                            self.ensure_reasoning(tx).await?;
                        }
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let delta = event.get("delta").unwrap_or(&Value::Null);
                match delta.get("type").and_then(Value::as_str).unwrap_or_default() {
                    "text_delta" => {
                        if let Some(text) = delta.get("text").and_then(Value::as_str).filter(|v| !v.is_empty()) {
                            self.text_delta(tx, text).await?;
                        }
                    }
                    "input_json_delta" => {
                        if let Some(arg_delta) = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .filter(|v| !v.is_empty())
                        {
                            self.tool_argument_delta(tx, index, arg_delta).await?;
                        }
                    }
                    "thinking_delta" => {
                        if let Some(text) = delta
                            .get("thinking")
                            .and_then(Value::as_str)
                            .filter(|v| !v.is_empty())
                        {
                            self.reasoning_delta(tx, text).await?;
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                if self.tool_calls.get(&index).map(|s| !s.closed).unwrap_or(false) {
                    self.close_tool(tx, index).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn reasoning_delta(&mut self, tx: &mpsc::Sender<StreamResult>, text: &str) -> AppResult<()> {
        self.ensure_reasoning(tx).await?;
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

    async fn ensure_reasoning(&mut self, tx: &mpsc::Sender<StreamResult>) -> AppResult<()> {
        if self.reasoning.is_some() {
            return Ok(());
        }
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
            let name = fn_obj.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            self.open_tool(tx, index, call_id, name).await?;
        } else if let Some(name) = fn_obj.get("name").and_then(Value::as_str).filter(|value| !value.is_empty()) {
            if let Some(state) = self.tool_calls.get_mut(&index) {
                state.name.push_str(name);
            }
        }
        let arg_delta = fn_obj.get("arguments").and_then(Value::as_str).unwrap_or("");
        if !arg_delta.is_empty() {
            self.tool_argument_delta(tx, index, arg_delta).await?;
        }
        Ok(())
    }

    async fn open_tool(
        &mut self,
        tx: &mpsc::Sender<StreamResult>,
        index: usize,
        call_id: String,
        name: String,
    ) -> AppResult<()> {
        if self.message_opened && !self.message_closed {
            self.close_message(tx).await?;
        }
        let output_index = self.next_output_index;
        self.next_output_index += 1;
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
        .await
    }

    async fn tool_argument_delta(
        &mut self,
        tx: &mpsc::Sender<StreamResult>,
        index: usize,
        arg_delta: &str,
    ) -> AppResult<()> {
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
        .await
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

fn decode_thinking_payload(encoded: &str) -> Option<Value> {
    const PREFIX: &str = "anthropic-thinking-v1:";
    let blob = encoded.strip_prefix(PREFIX)?;
    let raw = base64::engine::general_purpose::URL_SAFE
        .decode(blob.as_bytes())
        .ok()?;
    serde_json::from_slice(&raw).ok()
}

fn jsonish(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
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

fn anthropic_to_response(payload: Value, requested_model: &str) -> Value {
    chat_completion_to_response(anthropic_to_chat_response(payload, requested_model), requested_model)
}

fn anthropic_to_chat_response(payload: Value, requested_model: &str) -> Value {
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    if let Some(blocks) = payload.get("content").and_then(Value::as_array) {
        for block in blocks {
            match block.get("type").and_then(Value::as_str).unwrap_or_default() {
                "text" => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        content.push_str(text);
                    }
                }
                "tool_use" => {
                    tool_calls.push(json!({
                        "id": block.get("id").and_then(Value::as_str).unwrap_or("call_0"),
                        "type": "function",
                        "function": {
                            "name": block.get("name").and_then(Value::as_str).unwrap_or(""),
                            "arguments": jsonish(block.get("input").unwrap_or(&Value::Object(Map::new())))
                        }
                    }));
                }
                _ => {}
            }
        }
    }
    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    message.insert("content".to_string(), Value::String(strip_think(&content)));
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }
    json!({
        "id": payload.get("id").cloned().unwrap_or_else(|| Value::String("chatcmpl-anthropic".to_string())),
        "object": "chat.completion",
        "created": 0,
        "model": requested_model,
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": if payload.get("stop_reason").and_then(Value::as_str) == Some("tool_use") {
                "tool_calls"
            } else {
                "stop"
            }
        }]
    })
}

fn anthropic_stream_to_chat_chunk(event: &Value, model: &str) -> Value {
    let mut delta = Map::new();
    if event.get("type").and_then(Value::as_str) == Some("content_block_delta") {
        let event_delta = event.get("delta").unwrap_or(&Value::Null);
        if event_delta.get("type").and_then(Value::as_str) == Some("text_delta") {
            delta.insert(
                "content".to_string(),
                Value::String(event_delta.get("text").and_then(Value::as_str).unwrap_or("").to_string()),
            );
        }
    }
    json!({
        "object": "chat.completion.chunk",
        "model": model,
        "choices": [{"index": 0, "delta": Value::Object(delta), "finish_reason": Value::Null}]
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
            max_output_tokens: None,
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
            max_output_tokens: None,
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
    fn responses_to_anthropic_converts_tools_and_reasoning() {
        let route = RouteModel {
            slug: "claude-sonnet-4".to_string(),
            model: "claude-sonnet-4".to_string(),
            provider: "anthropic".to_string(),
            base_url: "https://api.anthropic.com/v1".to_string(),
            api_key: String::new(),
            max_output_tokens: Some(8192),
            extra_headers: HashMap::new(),
        };
        let thinking = encode_thinking_payload("prior thought");
        let out = responses_to_anthropic(
            &json!({
                "instructions": "system",
                "stream": true,
                "tools": [{"type":"function","name":"run","parameters":{"type":"object"}}],
                "input": [
                    {"type":"reasoning","summary":[{"type":"summary_text","text":"summary"}],"encrypted_content": thinking},
                    {"type":"message","role":"assistant","content":[{"type":"output_text","text":"hello"}]},
                    {"type":"function_call","call_id":"call_1","name":"run","arguments":"{\"cmd\":\"pwd\"}"},
                    {"type":"function_call_output","call_id":"call_1","output":"ok"}
                ]
            }),
            &route,
        );
        assert_eq!(out["stream"], Value::Bool(true));
        assert_eq!(out["system"], Value::String("system".to_string()));
        assert_eq!(out["tools"][0]["name"], "run");
        assert_eq!(out["messages"][0]["role"], "assistant");
        assert_eq!(out["messages"][0]["content"][0]["type"], "thinking");
        assert_eq!(out["messages"][0]["content"][1]["text"], "hello");
        assert_eq!(out["messages"][0]["content"][2]["type"], "tool_use");
        assert_eq!(out["messages"][1]["content"][0]["type"], "tool_result");
    }

    #[test]
    fn anthropic_to_response_converts_text_and_tool_use() {
        let out = anthropic_to_response(
            json!({
                "id": "msg_1",
                "content": [
                    {"type": "text", "text": "Answer"},
                    {"type": "tool_use", "id": "call_1", "name": "run", "input": {"cmd": "pwd"}}
                ],
                "stop_reason": "tool_use"
            }),
            "claude-sonnet-4",
        );
        assert_eq!(out["output"][0]["type"], "message");
        assert_eq!(out["output"][0]["content"][0]["text"], "Answer");
        assert_eq!(out["output"][1]["type"], "function_call");
        assert_eq!(out["output"][1]["arguments"], "{\"cmd\":\"pwd\"}");
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
    fn response_stream_state_accepts_anthropic_deltas() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (tx, mut rx) = mpsc::channel::<StreamResult>(32);
            let mut state = ResponsesStreamState::new("claude-sonnet-4".to_string());
            state.start(&tx).await.unwrap();
            state
                .write_anthropic_delta(
                    &tx,
                    &json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"think"}}),
                )
                .await
                .unwrap();
            state
                .write_anthropic_delta(
                    &tx,
                    &json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"hi"}}),
                )
                .await
                .unwrap();
            state
                .write_anthropic_delta(
                    &tx,
                    &json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"call_2","name":"run"}}),
                )
                .await
                .unwrap();
            state
                .write_anthropic_delta(
                    &tx,
                    &json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"cmd\""}}),
                )
                .await
                .unwrap();
            state
                .write_anthropic_delta(&tx, &json!({"type":"content_block_stop","index":2}))
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
            assert!(raw.contains("\"response.output_item.done\""));
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
