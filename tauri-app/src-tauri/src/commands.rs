use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::config::{self, AuthSnapshot};
use crate::embedded_shim::EmbeddedStatus;
use crate::error::{AppError, AppResult};
use crate::health::{self, HealthSnapshot};
use crate::models::{self, ModelsFile};
use crate::paths::{
    codex_auth_path, codex_config_path, default_settings_path, detect_project_root, log_path,
    DEFAULT_PORT,
};
use crate::shim::{CliOutput, ShimInvocation};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettingsDto {
    pub settings_path: String,
    pub port: u16,
    pub cli_override: Option<String>,
    pub project_root_override: Option<String>,
}

impl AppSettingsDto {
    fn from_state(state: &AppState) -> Self {
        let s = state.settings.lock().unwrap();
        Self {
            settings_path: s.settings_path.display().to_string(),
            port: s.port,
            cli_override: s.cli_override.clone(),
            project_root_override: s
                .project_root_override
                .as_ref()
                .map(|p| p.display().to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInfo {
    pub home_dir: String,
    pub default_settings_path: String,
    pub codex_auth_path: String,
    pub codex_config_path: String,
    pub detected_project_root: Option<String>,
    pub log_path: String,
    pub default_port: u16,
    pub platform: String,
}

#[tauri::command]
pub async fn get_runtime_info(state: State<'_, AppState>) -> AppResult<RuntimeInfo> {
    let override_root = {
        let s = state.settings.lock().unwrap();
        s.project_root_override.clone()
    };
    let detected = detect_project_root(override_root.as_deref());
    let logp = log_path(detected.as_deref());
    Ok(RuntimeInfo {
        home_dir: crate::paths::home_dir().display().to_string(),
        default_settings_path: default_settings_path().display().to_string(),
        codex_auth_path: codex_auth_path().display().to_string(),
        codex_config_path: codex_config_path().display().to_string(),
        detected_project_root: detected.map(|p| p.display().to_string()),
        log_path: logp.display().to_string(),
        default_port: DEFAULT_PORT,
        platform: std::env::consts::OS.to_string(),
    })
}

#[tauri::command]
pub async fn get_app_settings(state: State<'_, AppState>) -> AppResult<AppSettingsDto> {
    Ok(AppSettingsDto::from_state(&state))
}

#[tauri::command]
pub async fn update_app_settings(
    settings_path: Option<String>,
    port: Option<u16>,
    cli_override: Option<String>,
    project_root_override: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<AppSettingsDto> {
    {
        let mut s = state.settings.lock().unwrap();
        if let Some(path) = settings_path {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                s.settings_path = PathBuf::from(trimmed);
            }
        }
        if let Some(port) = port {
            s.port = port;
        }
        s.cli_override = cli_override
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        s.project_root_override = project_root_override
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
    }
    Ok(AppSettingsDto::from_state(&state))
}

fn current_settings(state: &State<'_, AppState>) -> (PathBuf, u16, Option<String>, Option<PathBuf>) {
    let s = state.settings.lock().unwrap();
    (
        s.settings_path.clone(),
        s.port,
        s.cli_override.clone(),
        s.project_root_override.clone(),
    )
}

async fn run_cli(state: &State<'_, AppState>, subcommand: &[&str]) -> AppResult<CliOutput> {
    let (settings_path, port, cli_override, project_root_override) = current_settings(state);
    let invocation = ShimInvocation::resolve(
        cli_override.as_deref(),
        project_root_override.as_deref(),
    )?;
    invocation.run(&settings_path, port, subcommand).await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShimStatus {
    pub cli: CliOutput,
    pub health: HealthSnapshot,
}

fn embedded_cli_output(status: EmbeddedStatus, args: &[&str], ok: bool) -> CliOutput {
    CliOutput {
        command: "embedded-shim".to_string(),
        args: args.iter().map(|v| (*v).to_string()).collect(),
        status: Some(if ok { 0 } else { 1 }),
        stdout: status.message,
        stderr: String::new(),
        ok,
    }
}

#[tauri::command]
pub async fn shim_status(state: State<'_, AppState>) -> AppResult<ShimStatus> {
    let port = current_settings(&state).1;
    let embedded = state.embedded_shim.status();
    let cli = embedded_cli_output(embedded, &["status"], true);
    let health = health::probe(port).await?;
    Ok(ShimStatus { cli, health })
}

#[tauri::command]
pub async fn shim_health(state: State<'_, AppState>) -> AppResult<HealthSnapshot> {
    let port = current_settings(&state).1;
    health::probe(port).await
}

#[tauri::command]
pub async fn shim_start(state: State<'_, AppState>) -> AppResult<CliOutput> {
    let (settings_path, port, _, _) = current_settings(&state);
    let status = state.embedded_shim.start(settings_path, port).await?;
    Ok(embedded_cli_output(status, &["start"], true))
}

#[tauri::command]
pub async fn shim_stop(state: State<'_, AppState>) -> AppResult<CliOutput> {
    let status = state.embedded_shim.stop();
    Ok(embedded_cli_output(status, &["stop"], true))
}

#[tauri::command]
pub async fn shim_restart(state: State<'_, AppState>) -> AppResult<CliOutput> {
    let (settings_path, port, _, _) = current_settings(&state);
    let _ = state.embedded_shim.stop();
    let status = state.embedded_shim.start(settings_path, port).await?;
    Ok(embedded_cli_output(status, &["restart"], true))
}

#[tauri::command]
pub async fn shim_generate(state: State<'_, AppState>) -> AppResult<CliOutput> {
    run_cli(&state, &["generate"]).await
}

#[tauri::command]
pub async fn shim_enable(state: State<'_, AppState>) -> AppResult<CliOutput> {
    run_cli(&state, &["enable"]).await
}

#[tauri::command]
pub async fn shim_disable(state: State<'_, AppState>) -> AppResult<CliOutput> {
    run_cli(&state, &["disable"]).await
}

#[tauri::command]
pub async fn shim_list_models(state: State<'_, AppState>) -> AppResult<CliOutput> {
    let path = current_settings(&state).0;
    let file = models::read_file(&path).await?;
    let mut rows: Vec<(String, String, String, String)> = Vec::new();
    let auth = config::read_codex_auth(&codex_auth_path()).await?;
    if auth.passthrough_available {
        rows.push(("gpt-5.5".to_string(), "GPT-5.5".to_string(), "gpt-5.5".to_string(), "chatgpt".to_string()));
    }
    rows.extend(file.models.iter().enumerate().filter_map(|(idx, row)| {
        if row.model.trim().is_empty() || row.provider.trim().is_empty() {
            return None;
        }
        let display = row.display_name.clone().unwrap_or_else(|| row.model.clone());
        let slug = models::slug_for_row(row, idx);
        Some((slug, display, row.model.clone(), row.provider.clone()))
    }));
    if rows.is_empty() {
        return Ok(CliOutput {
            command: "embedded-shim".to_string(),
            args: vec!["list".to_string()],
            status: Some(1),
            stdout: String::new(),
            stderr: "No models available. Create ~/.codex-shim/models.json or run codex login.".to_string(),
            ok: false,
        });
    }
    let width = rows.iter().map(|row| row.0.len()).max().unwrap_or(0);
    let stdout = rows
        .into_iter()
        .map(|(slug, display, model, provider)| {
            format!("{slug:<width$}  {display}  ->  {model} ({provider})")
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(CliOutput {
        command: "embedded-shim".to_string(),
        args: vec!["list".to_string()],
        status: Some(0),
        stdout: format!("{stdout}\n"),
        stderr: String::new(),
        ok: true,
    })
}

#[tauri::command]
pub async fn shim_use_model(slug: String, state: State<'_, AppState>) -> AppResult<CliOutput> {
    if slug.trim().is_empty() {
        return Err(AppError::msg("slug 不能为空"));
    }
    let (_, port, _, _) = current_settings(&state);
    config::install_codex_model_config(&codex_config_path(), slug.trim(), port).await?;
    Ok(CliOutput {
        command: "embedded-shim".to_string(),
        args: vec!["model".to_string(), "use".to_string(), slug],
        status: Some(0),
        stdout: "Updated ~/.codex/config.toml managed model config.\n".to_string(),
        stderr: String::new(),
        ok: true,
    })
}

#[tauri::command]
pub async fn shim_launch_app(path: Option<String>, state: State<'_, AppState>) -> AppResult<CliOutput> {
    let path = path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    run_cli(&state, &["app", path]).await
}

#[tauri::command]
pub async fn shim_patch_app(state: State<'_, AppState>) -> AppResult<CliOutput> {
    run_cli(&state, &["patch-app"]).await
}

#[tauri::command]
pub async fn shim_restore_app(state: State<'_, AppState>) -> AppResult<CliOutput> {
    run_cli(&state, &["restore-app"]).await
}

#[tauri::command]
pub async fn read_models_file(state: State<'_, AppState>) -> AppResult<ModelsFile> {
    let path = current_settings(&state).0;
    models::read_file(&path).await
}

#[tauri::command]
pub async fn write_models_file(
    file: ModelsFile,
    state: State<'_, AppState>,
) -> AppResult<ModelsFile> {
    for row in &file.models {
        models::validate(row)?;
    }
    let path = current_settings(&state).0;
    models::write_file(&path, &file).await?;
    models::read_file(&path).await
}

#[tauri::command]
pub async fn tail_log(state: State<'_, AppState>, max_bytes: Option<usize>) -> AppResult<String> {
    let project = current_settings(&state).3;
    let detected = detect_project_root(project.as_deref());
    let path = log_path(detected.as_deref());
    let bytes = max_bytes.unwrap_or(64 * 1024).min(1024 * 1024);
    config::tail_log(&path, bytes).await
}

#[tauri::command]
pub async fn read_codex_auth() -> AppResult<AuthSnapshot> {
    config::read_codex_auth(&codex_auth_path()).await
}

#[tauri::command]
pub async fn current_active_model() -> AppResult<Option<String>> {
    config::read_active_model(&codex_config_path()).await
}
