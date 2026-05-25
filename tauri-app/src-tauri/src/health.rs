use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AppResult;
use crate::paths::DEFAULT_HOST;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HealthSnapshot {
    pub ok: bool,
    pub url: String,
    pub status: Option<u16>,
    pub models: Option<u64>,
    pub raw: Option<Value>,
    pub error: Option<String>,
}

pub async fn probe(port: u16) -> AppResult<HealthSnapshot> {
    let url = format!("http://{DEFAULT_HOST}:{port}/health");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()?;
    let snapshot = match client.get(&url).send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            match resp.json::<Value>().await {
                Ok(raw) => HealthSnapshot {
                    ok: status == 200,
                    url,
                    status: Some(status),
                    models: raw
                        .get("models")
                        .and_then(|v| v.as_u64())
                        .or_else(|| raw.get("models").and_then(|v| v.as_i64()).map(|n| n as u64)),
                    raw: Some(raw),
                    error: None,
                },
                Err(err) => HealthSnapshot {
                    ok: false,
                    url,
                    status: Some(status),
                    models: None,
                    raw: None,
                    error: Some(err.to_string()),
                },
            }
        }
        Err(err) => HealthSnapshot {
            ok: false,
            url,
            status: None,
            models: None,
            raw: None,
            error: Some(err.to_string()),
        },
    };
    Ok(snapshot)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelListing {
    pub raw: Value,
}

pub async fn list_models(port: u16) -> AppResult<ModelListing> {
    let url = format!("http://{DEFAULT_HOST}:{port}/v1/models");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;
    let raw: Value = client.get(url).send().await?.json().await?;
    Ok(ModelListing { raw })
}
