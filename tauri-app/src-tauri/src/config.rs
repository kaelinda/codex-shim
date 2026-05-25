use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs;

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthSnapshot {
    pub auth_path: String,
    pub exists: bool,
    pub passthrough_available: bool,
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub plan: Option<String>,
}

pub async fn read_codex_auth(path: &Path) -> AppResult<AuthSnapshot> {
    let mut snap = AuthSnapshot {
        auth_path: path.display().to_string(),
        ..Default::default()
    };
    if !path.exists() {
        return Ok(snap);
    }
    snap.exists = true;
    let text = match fs::read_to_string(path).await {
        Ok(t) => t,
        Err(_) => return Ok(snap),
    };
    let parsed: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return Ok(snap),
    };
    let tokens = parsed.get("tokens").and_then(|v| v.as_object());
    let has_access_token = tokens
        .and_then(|t| t.get("access_token"))
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    snap.passthrough_available = has_access_token;
    snap.account_id = parsed
        .get("account_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            tokens
                .and_then(|t| t.get("account_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });
    snap.email = parsed
        .get("email")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            tokens
                .and_then(|t| t.get("email"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });
    snap.plan = parsed
        .get("plan_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Ok(snap)
}

/// Read the `model = "..."` line inside the managed block of
/// `~/.codex/config.toml`. We deliberately do not do a full TOML parse: the
/// managed block is line-oriented and other entries may legitimately use
/// unquoted bare keys we do not care about here.
pub async fn read_active_model(config_path: &Path) -> AppResult<Option<String>> {
    if !config_path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(config_path).await?;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with("model = ") || line.starts_with("model=") {
            let value = line.splitn(2, '=').nth(1).unwrap_or("").trim();
            let trimmed = value.trim_matches('"');
            if !trimmed.is_empty() {
                return Ok(Some(trimmed.to_string()));
            }
        }
    }
    Ok(None)
}

pub async fn tail_log(path: &Path, max_bytes: usize) -> AppResult<String> {
    if !path.exists() {
        return Ok(String::new());
    }
    let text = fs::read_to_string(path).await?;
    if text.len() <= max_bytes {
        return Ok(text);
    }
    let start = text.len().saturating_sub(max_bytes);
    // Avoid splitting a multibyte char by walking forward to the next boundary.
    let mut idx = start;
    while idx < text.len() && !text.is_char_boundary(idx) {
        idx += 1;
    }
    Ok(text[idx..].to_string())
}
