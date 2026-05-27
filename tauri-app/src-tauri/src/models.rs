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

#[derive(Debug, Clone)]
pub struct ShimModel {
    pub slug: String,
    pub model: String,
    pub display_name: String,
    pub provider: String,
    pub index: usize,
    pub max_context_limit: Option<i64>,
    pub no_image_support: bool,
}

pub fn model_rows(file: &ModelsFile) -> Vec<ShimModel> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for row in &file.models {
        let model = row.model.trim();
        if !model.is_empty() {
            *counts.entry(model.to_string()).or_default() += 1;
        }
    }

    let mut used = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (fallback_index, row) in file.models.iter().enumerate() {
        let model = row.model.trim();
        let provider = row.provider.trim();
        if model.is_empty() || provider.is_empty() || row.base_url.trim().is_empty() {
            continue;
        }
        let index = row
            .extra
            .get("index")
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(fallback_index);
        let display_name = row
            .display_name
            .as_deref()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or(model)
            .trim()
            .to_string();
        let slug_base = row
            .extra
            .get("slug")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                if counts.get(model).copied().unwrap_or(0) > 1 {
                    display_name.clone()
                } else {
                    model.to_string()
                }
            });
        let mut slug = slugify(&slug_base);
        if used.contains(&slug) {
            slug = format!("{slug}-{index}");
        }
        while used.contains(&slug) {
            slug = format!("{slug}-{}", used.len());
        }
        used.insert(slug.clone());
        out.push(ShimModel {
            slug,
            model: model.to_string(),
            display_name,
            provider: provider.to_string(),
            index,
            max_context_limit: row.max_context_limit,
            no_image_support: row.no_image_support,
        });
    }
    out
}

pub fn default_model_slug(models: &[ShimModel], passthrough_available: bool) -> String {
    if passthrough_available {
        "gpt-5.5".to_string()
    } else {
        models
            .first()
            .map(|model| model.slug.clone())
            .unwrap_or_else(|| "gpt-5.5".to_string())
    }
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
    slug.trim_matches('-').to_string()
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_custom_openai_compatible_provider() {
        let row = ModelRow {
            model: "gpt-4.1".to_string(),
            provider: "new-api".to_string(),
            base_url: "https://new-api.example.com/v1".to_string(),
            ..Default::default()
        };
        assert!(validate(&row).is_ok());
    }
}
