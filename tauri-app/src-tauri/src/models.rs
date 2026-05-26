use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs;

use crate::error::{AppError, AppResult};

/// One row inside `~/.codex-shim/models.json` -> `models[]`.
///
/// We intentionally keep this loose: codex-shim accepts both snake_case and
/// camelCase keys, and users sometimes carry over historical fields. The GUI
/// owns the *canonical* shape it writes back, but on read we tolerate extras
/// and surface them via `extra` so they round-trip unchanged.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelRow {
    pub model: String,
    pub provider: String,
    #[serde(alias = "baseUrl")]
    pub base_url: String,
    #[serde(default, alias = "apiKey")]
    pub api_key: String,
    #[serde(default, alias = "displayName", skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, alias = "maxContextLimit", skip_serializing_if = "Option::is_none")]
    pub max_context_limit: Option<i64>,
    #[serde(default, alias = "maxOutputTokens", skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i64>,
    #[serde(default, alias = "noImageSupport", skip_serializing_if = "is_false")]
    pub no_image_support: bool,
    #[serde(default, alias = "extraHeaders", skip_serializing_if = "Option::is_none")]
    pub extra_headers: Option<serde_json::Map<String, Value>>,
    /// Catch-all bucket for fields we do not model (e.g. legacy/custom keys).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelsFile {
    #[serde(default)]
    pub models: Vec<ModelRow>,
    /// Any other top-level keys (e.g. `customModels`) we preserve on write.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

pub async fn read_file(path: &Path) -> AppResult<ModelsFile> {
    if !path.exists() {
        return Ok(ModelsFile::default());
    }
    let text = fs::read_to_string(path).await?;
    if text.trim().is_empty() {
        return Ok(ModelsFile::default());
    }
    // Accept legacy `customModels` arrays as well; promote them into `models`.
    let raw: Value = serde_json::from_str(&text)?;
    let mut file: ModelsFile = serde_json::from_value(raw.clone())?;
    if file.models.is_empty() {
        if let Some(Value::Array(rows)) = raw.get("customModels") {
            let migrated: Result<Vec<ModelRow>, _> = rows
                .iter()
                .map(|row| serde_json::from_value::<ModelRow>(row.clone()))
                .collect();
            if let Ok(rows) = migrated {
                file.models = rows;
            }
        }
    }
    Ok(file)
}

pub async fn write_file(path: &Path, file: &ModelsFile) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).await?;
        }
    }
    let serialized = serde_json::to_string_pretty(file)?;
    fs::write(path, format!("{serialized}\n")).await?;
    Ok(())
}

/// Sanity-check a row before writing it: required fields must be present.
pub fn validate(row: &ModelRow) -> AppResult<()> {
    if row.model.trim().is_empty() {
        return Err(AppError::msg("model 不能为空"));
    }
    if row.provider.trim().is_empty() {
        return Err(AppError::msg("provider 不能为空"));
    }
    if row.base_url.trim().is_empty() {
        return Err(AppError::msg("base_url 不能为空"));
    }
    match row.provider.as_str() {
        "openai" | "anthropic" | "generic-chat-completion-api" | "deepseek" => Ok(()),
        other => Err(AppError::msg(format!(
            "未知 provider {other:?}，支持: openai / anthropic / generic-chat-completion-api / deepseek"
        ))),
    }
}
