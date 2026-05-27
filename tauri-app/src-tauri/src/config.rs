use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs;

use crate::catalog;
use crate::error::{AppError, AppResult};
use crate::models::ShimModel;
use crate::paths::codex_config_backup_path;

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

pub async fn install_codex_config(
    config_path: &Path,
    models: &[ShimModel],
    passthrough_available: bool,
    model_slug: &str,
    port: u16,
) -> AppResult<()> {
    if !passthrough_available && !models.iter().any(|model| model.slug == model_slug || model.model == model_slug) {
        return Err(AppError::msg(format!("Unknown shim model {model_slug:?}")));
    }
    let selected = models
        .iter()
        .find(|model| model.slug == model_slug || model.model == model_slug)
        .map(|model| model.slug.clone())
        .unwrap_or_else(|| model_slug.to_string());
    let original = if config_path.exists() {
        fs::read_to_string(config_path).await?
    } else {
        String::new()
    };
    let backup_path = codex_config_backup_path();
    if !original.contains(MANAGED_BEGIN) && !backup_path.exists() {
        if let Some(parent) = backup_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&backup_path, &original).await?;
    }
    let cleaned = remove_managed_config(&original);
    let cleaned = remove_top_level_keys(&cleaned, &["model", "model_provider", "model_catalog_json"]);
    let cleaned = remove_section(&cleaned, "model_providers.codex_shim");
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let catalog_path = crate::paths::catalog_path();
    let block = format!(
        r#"{MANAGED_BEGIN}
model = "{}"
model_provider = "codex_shim"
model_catalog_json = "{}"
{MANAGED_END}

{MANAGED_BEGIN}
[model_providers.codex_shim]
name = "Codex Shim"
base_url = "http://127.0.0.1:{port}/v1"
wire_api = "responses"
experimental_bearer_token = "dummy"
request_max_retries = 3
stream_max_retries = 3
stream_idle_timeout_ms = 600000
{MANAGED_END}
"#,
        catalog::toml_escape(&selected),
        catalog::toml_escape(&catalog_path.display().to_string()),
    );
    fs::write(config_path, format!("{block}\n{}", cleaned.trim_start())).await?;
    Ok(())
}

pub async fn restore_codex_config(config_path: &Path) -> AppResult<bool> {
    let backup_path = codex_config_backup_path();
    if backup_path.exists() {
        let backup = fs::read_to_string(&backup_path).await?;
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(config_path, backup).await?;
        fs::remove_file(backup_path).await?;
        return Ok(true);
    }
    if config_path.exists() {
        let current = fs::read_to_string(config_path).await?;
        let restored = remove_section(&remove_managed_config(&current), "model_providers.codex_shim");
        fs::write(config_path, restored.trim_start()).await?;
        return Ok(false);
    }
    Ok(false)
}

const MANAGED_BEGIN: &str = "# >>> codex-shim managed";
const MANAGED_END: &str = "# <<< codex-shim managed";

fn remove_managed_config(text: &str) -> String {
    let mut current = text.to_string();
    while let Some(start) = current.find(MANAGED_BEGIN) {
        let Some(end_rel) = current[start..].find(MANAGED_END) else {
            current.truncate(start);
            break;
        };
        let end = start + end_rel + MANAGED_END.len();
        current.replace_range(start..end, "");
    }
    current
}

fn remove_top_level_keys(text: &str, keys: &[&str]) -> String {
    let mut output = Vec::new();
    let mut in_top_level = true;
    for line in text.lines() {
        let stripped = line.trim();
        if stripped.starts_with('[') {
            in_top_level = false;
        }
        let key = stripped.split_once('=').map(|(key, _)| key.trim());
        if in_top_level && key.map(|key| keys.contains(&key)).unwrap_or(false) {
            continue;
        }
        output.push(line);
    }
    let mut joined = output.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

fn remove_section(text: &str, section: &str) -> String {
    let mut output = Vec::new();
    let mut skipping = false;
    let header = format!("[{section}]");
    for line in text.lines() {
        let stripped = line.trim();
        if stripped.starts_with('[') && stripped.ends_with(']') {
            skipping = stripped == header;
            if skipping {
                continue;
            }
        }
        if !skipping {
            output.push(line);
        }
    }
    let mut joined = output.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    joined
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
