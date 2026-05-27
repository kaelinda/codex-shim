use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{HeaderName, HeaderValue, CONTENT_TYPE};
use hyper::server::conn::http1;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::config;
use crate::error::{AppError, AppResult};
use crate::models;
use crate::paths::{codex_auth_path, DEFAULT_HOST};

type RespBody = Full<Bytes>;

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
        let upstream = post_openai_chat(ctx, &route, forwarded).await?;
        return Ok(json_response(
            StatusCode::OK,
            chat_completion_to_response(upstream, &route.slug),
        ));
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
    let upstream = post_openai_chat(ctx, &route, body).await?;
    Ok(json_response(StatusCode::OK, upstream))
}

async fn post_openai_chat(ctx: &ServerContext, route: &RouteModel, body: Value) -> AppResult<Value> {
    if body.get("stream").and_then(Value::as_bool).unwrap_or(false) {
        return Err(AppError::msg("Embedded Rust shim currently supports non-streaming requests only."));
    }
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
    let text = response.text().await?;
    if !status.is_success() {
        return Err(AppError::msg(format!("upstream returned {status}: {text}")));
    }
    serde_json::from_str(&text).map_err(AppError::from)
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
    chat.insert("stream".to_string(), Value::Bool(false));
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
            "encrypted_content": Value::Null
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
    let mut response = Response::new(Full::new(Bytes::from(value.to_string())));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

fn text_response(status: StatusCode, text: impl Into<String>) -> Response<RespBody> {
    let mut response = Response::new(Full::new(Bytes::from(text.into())));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
    response
}

fn error_response(err: AppError) -> Response<RespBody> {
    text_response(StatusCode::BAD_GATEWAY, err.to_string())
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
        assert_eq!(out["stream"], Value::Bool(false));
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
        assert_eq!(out["output"][1]["content"][0]["text"], "Answer");
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
