use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;
use tokio::fs::{self, File};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::catalog;
use crate::config::{self, AuthSnapshot};
use crate::embedded_shim::EmbeddedStatus;
use crate::error::{AppError, AppResult};
use crate::health::{self, HealthSnapshot};
use crate::models::{self, ModelsFile};
use crate::paths::{
    app_runtime_dir, catalog_path, codex_auth_path, codex_config_path, default_settings_path,
    generated_config_path, log_path, DEFAULT_PORT,
};
use crate::state::AppState;
use crate::updater::{self, UpdateInfo};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliOutput {
    pub command: String,
    pub args: Vec<String>,
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigTransferResult {
    pub path: String,
    pub backup_path: Option<String>,
    pub model_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettingsDto {
    pub settings_path: String,
    pub port: u16,
}

impl AppSettingsDto {
    fn from_state(state: &AppState) -> Self {
        let s = state.settings.lock().unwrap();
        Self {
            settings_path: s.settings_path.display().to_string(),
            port: s.port,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInfo {
    pub home_dir: String,
    pub default_settings_path: String,
    pub codex_auth_path: String,
    pub codex_config_path: String,
    pub log_path: String,
    pub default_port: u16,
    pub platform: String,
    pub app_version: String,
}

#[tauri::command]
pub async fn get_runtime_info(_state: State<'_, AppState>) -> AppResult<RuntimeInfo> {
    let logp = log_path();
    Ok(RuntimeInfo {
        home_dir: crate::paths::home_dir().display().to_string(),
        default_settings_path: default_settings_path().display().to_string(),
        codex_auth_path: codex_auth_path().display().to_string(),
        codex_config_path: codex_config_path().display().to_string(),
        log_path: logp.display().to_string(),
        default_port: DEFAULT_PORT,
        platform: std::env::consts::OS.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
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
    }
    Ok(AppSettingsDto::from_state(&state))
}

fn current_settings(state: &State<'_, AppState>) -> (PathBuf, u16) {
    let s = state.settings.lock().unwrap();
    (s.settings_path.clone(), s.port)
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

async fn generate_catalog_and_config(state: &State<'_, AppState>) -> AppResult<CliOutput> {
    let (settings_path, port) = current_settings(state);
    let file = models::read_file(&settings_path).await?;
    let rows = models::model_rows(&file);
    let auth = config::read_codex_auth(&codex_auth_path()).await?;
    let catalog_path = catalog_path();
    let config_path = generated_config_path();
    catalog::write_catalog(&rows, &catalog_path, auth.passthrough_available).await?;
    catalog::write_generated_config(
        &rows,
        &config_path,
        &catalog_path,
        port,
        auth.passthrough_available,
    )
    .await?;
    Ok(CliOutput {
        command: "embedded-shim".to_string(),
        args: vec!["generate".to_string()],
        status: Some(0),
        stdout: format!(
            "Generated {} model entries:\n  catalog: {}\n  config:  {}\nNo files under ~/.codex were modified.\n",
            rows.len(),
            catalog_path.display(),
            config_path.display()
        ),
        stderr: String::new(),
        ok: true,
    })
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
    let (settings_path, port) = current_settings(&state);
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
    let (settings_path, port) = current_settings(&state);
    let _ = state.embedded_shim.stop();
    let status = state.embedded_shim.start(settings_path, port).await?;
    Ok(embedded_cli_output(status, &["restart"], true))
}

#[tauri::command]
pub async fn shim_generate(state: State<'_, AppState>) -> AppResult<CliOutput> {
    generate_catalog_and_config(&state).await
}

#[tauri::command]
pub async fn shim_enable(state: State<'_, AppState>) -> AppResult<CliOutput> {
    let (settings_path, port) = current_settings(&state);
    let file = models::read_file(&settings_path).await?;
    let rows = models::model_rows(&file);
    let auth = config::read_codex_auth(&codex_auth_path()).await?;
    let catalog_path = catalog_path();
    let config_path = generated_config_path();
    catalog::write_catalog(&rows, &catalog_path, auth.passthrough_available).await?;
    catalog::write_generated_config(
        &rows,
        &config_path,
        &catalog_path,
        port,
        auth.passthrough_available,
    )
    .await?;
    let default_slug = models::default_model_slug(&rows, auth.passthrough_available);
    config::install_codex_config(
        &codex_config_path(),
        &rows,
        auth.passthrough_available,
        &default_slug,
        port,
    )
    .await?;
    let status = state.embedded_shim.start(settings_path, port).await?;
    Ok(CliOutput {
        command: "embedded-shim".to_string(),
        args: vec!["enable".to_string()],
        status: Some(0),
        stdout: format!(
            "Generated catalog/config, installed ~/.codex config, and ensured embedded shim is running.\n{}\n",
            status.message
        ),
        stderr: String::new(),
        ok: true,
    })
}

#[tauri::command]
pub async fn shim_disable(state: State<'_, AppState>) -> AppResult<CliOutput> {
    let restored_backup = config::restore_codex_config(&codex_config_path()).await?;
    let status = state.embedded_shim.stop();
    Ok(CliOutput {
        command: "embedded-shim".to_string(),
        args: vec!["disable".to_string()],
        status: Some(0),
        stdout: format!(
            "{}\n{}\n",
            if restored_backup {
                "Restored original ~/.codex/config.toml."
            } else {
                "Removed codex-shim managed config from ~/.codex/config.toml."
            },
            status.message
        ),
        stderr: String::new(),
        ok: true,
    })
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
    rows.extend(
        models::model_rows(&file)
            .into_iter()
            .map(|model| (model.slug, model.display_name, model.model, model.provider)),
    );
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
    let (settings_path, port) = current_settings(&state);
    let file = models::read_file(&settings_path).await?;
    let rows = models::model_rows(&file);
    let auth = config::read_codex_auth(&codex_auth_path()).await?;
    catalog::write_catalog(&rows, &catalog_path(), auth.passthrough_available).await?;
    catalog::write_generated_config(
        &rows,
        &generated_config_path(),
        &catalog_path(),
        port,
        auth.passthrough_available,
    )
    .await?;
    config::install_codex_config(
        &codex_config_path(),
        &rows,
        auth.passthrough_available,
        slug.trim(),
        port,
    )
    .await?;
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
    let (settings_path, port) = current_settings(&state);
    let file = models::read_file(&settings_path).await?;
    let rows = models::model_rows(&file);
    let auth = config::read_codex_auth(&codex_auth_path()).await?;
    catalog::write_catalog(&rows, &catalog_path(), auth.passthrough_available).await?;
    catalog::write_generated_config(
        &rows,
        &generated_config_path(),
        &catalog_path(),
        port,
        auth.passthrough_available,
    )
    .await?;
    let default_slug = models::default_model_slug(&rows, auth.passthrough_available);
    config::install_codex_config(
        &codex_config_path(),
        &rows,
        auth.passthrough_available,
        &default_slug,
        port,
    )
    .await?;
    let status = state.embedded_shim.start(settings_path, port).await?;
    let _ = quit_codex_app().await;
    launch_codex_desktop(path).await?;
    let _ = foreground_codex_app().await;
    Ok(CliOutput {
        command: "rust-launcher".to_string(),
        args: vec!["app".to_string(), path.to_string()],
        status: Some(0),
        stdout: format!("Launched Codex Desktop and ensured embedded shim is running.\n{}\n", status.message),
        stderr: String::new(),
        ok: true,
    })
}

#[tauri::command]
pub async fn shim_patch_app(_state: State<'_, AppState>) -> AppResult<CliOutput> {
    patch_codex_app().await
}

#[tauri::command]
pub async fn shim_restore_app(_state: State<'_, AppState>) -> AppResult<CliOutput> {
    restore_codex_app_bundle().await
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
pub async fn export_models_file(
    path: String,
    without_keys: bool,
    state: State<'_, AppState>,
) -> AppResult<ConfigTransferResult> {
    let export_path = PathBuf::from(path.trim());
    if export_path.as_os_str().is_empty() {
        return Err(AppError::msg("导出路径不能为空"));
    }
    let settings_path = current_settings(&state).0;
    let mut file = models::read_file(&settings_path).await?;
    validate_models_file(&file)?;
    if without_keys {
        for row in &mut file.models {
            row.api_key.clear();
        }
    }
    models::write_file(&export_path, &file).await?;
    Ok(ConfigTransferResult {
        path: export_path.display().to_string(),
        backup_path: None,
        model_count: file.models.len(),
    })
}

#[tauri::command]
pub async fn import_models_file(
    path: String,
    state: State<'_, AppState>,
) -> AppResult<ConfigTransferResult> {
    let import_path = PathBuf::from(path.trim());
    if import_path.as_os_str().is_empty() {
        return Err(AppError::msg("导入路径不能为空"));
    }
    let file = models::read_file(&import_path).await?;
    validate_models_file(&file)?;
    if file.models.is_empty() {
        return Err(AppError::msg("导入文件中没有任何 provider 配置。"));
    }
    let settings_path = current_settings(&state).0;
    let backup_path = if settings_path.exists() {
        let backup = backup_path_for(&settings_path)?;
        if let Some(parent) = backup.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).await?;
            }
        }
        fs::copy(&settings_path, &backup).await?;
        Some(backup)
    } else {
        None
    };
    models::write_file(&settings_path, &file).await?;
    Ok(ConfigTransferResult {
        path: settings_path.display().to_string(),
        backup_path: backup_path.map(|path| path.display().to_string()),
        model_count: file.models.len(),
    })
}

fn validate_models_file(file: &ModelsFile) -> AppResult<()> {
    for row in &file.models {
        models::validate(row)?;
    }
    Ok(())
}

fn backup_path_for(path: &Path) -> AppResult<PathBuf> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| AppError::msg(format!("系统时间异常：{err}")))?
        .as_secs();
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("models.json");
    Ok(path.with_file_name(format!("{filename}.bak.{timestamp}")))
}

#[tauri::command]
pub async fn tail_log(state: State<'_, AppState>, max_bytes: Option<usize>) -> AppResult<String> {
    let _ = state;
    let path = log_path();
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

#[tauri::command]
pub async fn check_app_update() -> AppResult<UpdateInfo> {
    updater::check_latest_release().await
}

#[tauri::command]
pub async fn install_cli_update(ref_name: Option<String>) -> AppResult<CliOutput> {
    let output = updater::install_cli_update(ref_name.as_deref()).await?;
    Ok(CliOutput {
        command: output.command,
        args: output.args,
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
        ok: output.ok,
    })
}

async fn quit_codex_app() -> AppResult<()> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    run_quiet(
        "osascript",
        &["-e", "tell application \"Codex\" to if it is running then quit"],
    )
    .await?;
    Ok(())
}

async fn foreground_codex_app() -> AppResult<()> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    let script = r#"
tell application "Codex" to activate
delay 0.5
tell application "System Events"
  if exists process "Codex" then
    tell process "Codex"
      set frontmost to true
      if (count of windows) is 0 then
        keystroke "n" using command down
        delay 0.3
      end if
      if (count of windows) > 0 then
        set position of window 1 to {80, 60}
        set size of window 1 to {1400, 980}
      end if
    end tell
  end if
end tell
"#;
    run_quiet("osascript", &["-e", script]).await?;
    Ok(())
}

async fn launch_codex_desktop(path: &str) -> AppResult<()> {
    if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command
            .args(["-a", "Codex", path])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command.spawn()?;
        Ok(())
    } else {
        Err(AppError::msg(
            "Codex Desktop launch is currently implemented for macOS only.",
        ))
    }
}

async fn patch_codex_app() -> AppResult<CliOutput> {
    if !cfg!(target_os = "macos") {
        return Ok(CliOutput {
            command: "rust-patch-app".to_string(),
            args: vec!["patch-app".to_string()],
            status: Some(1),
            stdout: String::new(),
            stderr: "Codex Desktop picker patch is only supported on macOS.".to_string(),
            ok: false,
        });
    }
    let app_asar = PathBuf::from("/Applications/Codex.app/Contents/Resources/app.asar");
    let runtime = app_runtime_dir();
    let backup = runtime.join("app.asar.before-codex-shim-model-picker-patch");
    let workdir = runtime.join("app-asar-work");
    let needle = "let u=c.useHiddenModels&&o!==`amazonBedrock`,d;";
    let replacement = "let u=!1,d;";
    if !app_asar.exists() {
        return Ok(CliOutput {
            command: "rust-patch-app".to_string(),
            args: vec!["patch-app".to_string()],
            status: Some(1),
            stdout: String::new(),
            stderr: format!("Codex app bundle not found at {}.", app_asar.display()),
            ok: false,
        });
    }
    if which::which("npx").is_err() {
        return Ok(CliOutput {
            command: "rust-patch-app".to_string(),
            args: vec!["patch-app".to_string()],
            status: Some(1),
            stdout: String::new(),
            stderr: "npx is required to patch the Electron asar bundle.".to_string(),
            ok: false,
        });
    }
    fs::create_dir_all(&runtime).await?;
    let mut stdout = String::new();
    if !backup.exists() {
        fs::copy(&app_asar, &backup).await?;
        stdout.push_str(&format!("Backed up original app.asar to {}.\n", backup.display()));
    }
    let hash = file_sha256(&app_asar).await?;
    let versioned_backup = runtime.join(format!(
        "app.asar.before-codex-shim-model-picker-patch.{}",
        &hash[..12]
    ));
    if !versioned_backup.exists() {
        fs::copy(&app_asar, &versioned_backup).await?;
        stdout.push_str(&format!("Backed up current app.asar to {}.\n", versioned_backup.display()));
    }
    let _ = quit_codex_app().await;
    if workdir.exists() {
        fs::remove_dir_all(&workdir).await?;
    }
    fs::create_dir_all(&workdir).await?;
    let extract = run_command(
        "npx",
        &["--yes", "asar", "extract", &app_asar.display().to_string(), &workdir.display().to_string()],
    )
    .await?;
    if !extract.ok {
        return Ok(extract);
    }
    let Some(bundle_file) = find_model_queries_bundle(&workdir, needle, replacement).await? else {
        return Ok(CliOutput {
            command: "rust-patch-app".to_string(),
            args: vec!["patch-app".to_string()],
            status: Some(1),
            stdout,
            stderr: "Could not find the expected model picker filter in Codex Desktop.".to_string(),
            ok: false,
        });
    };
    let text = fs::read_to_string(&bundle_file).await?;
    if text.contains(replacement) {
        stdout.push_str("Codex Desktop model picker patch is already applied.\n");
    } else if text.contains(needle) {
        fs::write(&bundle_file, text.replace(needle, replacement)).await?;
        let pack = run_command(
            "npx",
            &["--yes", "asar", "pack", &workdir.display().to_string(), &app_asar.display().to_string()],
        )
        .await?;
        if !pack.ok {
            return Ok(pack);
        }
        stdout.push_str("Patched Codex Desktop model picker allowlist filter.\n");
        let resign = run_command(
            "codesign",
            &["--force", "--deep", "--sign", "-", "/Applications/Codex.app"],
        )
        .await?;
        if resign.ok {
            stdout.push_str("Re-signed Codex.app after patch.\n");
        } else {
            return Ok(resign);
        }
    } else {
        return Ok(CliOutput {
            command: "rust-patch-app".to_string(),
            args: vec!["patch-app".to_string()],
            status: Some(1),
            stdout,
            stderr: "Could not find the expected model picker filter in Codex Desktop.".to_string(),
            ok: false,
        });
    }
    Ok(CliOutput {
        command: "rust-patch-app".to_string(),
        args: vec!["patch-app".to_string()],
        status: Some(0),
        stdout,
        stderr: String::new(),
        ok: true,
    })
}

async fn restore_codex_app_bundle() -> AppResult<CliOutput> {
    if !cfg!(target_os = "macos") {
        return Ok(CliOutput {
            command: "rust-restore-app".to_string(),
            args: vec!["restore-app".to_string()],
            status: Some(1),
            stdout: String::new(),
            stderr: "Codex Desktop picker restore is only supported on macOS.".to_string(),
            ok: false,
        });
    }
    let app_asar = PathBuf::from("/Applications/Codex.app/Contents/Resources/app.asar");
    let backup = app_runtime_dir().join("app.asar.before-codex-shim-model-picker-patch");
    if !backup.exists() {
        return Ok(CliOutput {
            command: "rust-restore-app".to_string(),
            args: vec!["restore-app".to_string()],
            status: Some(0),
            stdout: format!("No app.asar backup found at {}.\n", backup.display()),
            stderr: String::new(),
            ok: true,
        });
    }
    let _ = quit_codex_app().await;
    fs::copy(&backup, &app_asar).await?;
    Ok(CliOutput {
        command: "rust-restore-app".to_string(),
        args: vec!["restore-app".to_string()],
        status: Some(0),
        stdout: format!("Restored {} from {}.\n", app_asar.display(), backup.display()),
        stderr: String::new(),
        ok: true,
    })
}

async fn run_quiet(program: &str, args: &[&str]) -> AppResult<CliOutput> {
    let mut command = Command::new(program);
    command.args(args).stdout(Stdio::null()).stderr(Stdio::null());
    let status = command.status().await?;
    Ok(CliOutput {
        command: program.to_string(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
        status: status.code(),
        stdout: String::new(),
        stderr: String::new(),
        ok: status.success(),
    })
}

async fn run_command(program: &str, args: &[&str]) -> AppResult<CliOutput> {
    let output = Command::new(program).args(args).output().await?;
    Ok(CliOutput {
        command: program.to_string(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        ok: output.status.success(),
    })
}

async fn file_sha256(path: &Path) -> AppResult<String> {
    let mut file = File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

async fn find_model_queries_bundle(
    workdir: &Path,
    needle: &str,
    replacement: &str,
) -> AppResult<Option<PathBuf>> {
    let assets_dir = workdir.join("webview").join("assets");
    if !assets_dir.exists() {
        return Ok(None);
    }
    let mut read_dir = fs::read_dir(&assets_dir).await?;
    let mut candidates = Vec::new();
    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("js") {
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
            let priority = if name.starts_with("model-queries-") { 0 } else { 1 };
            candidates.push((priority, path));
        }
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    for (_, path) in candidates {
        let text = fs::read_to_string(&path).await.unwrap_or_default();
        if text.contains(needle) || text.contains(replacement) {
            return Ok(Some(path));
        }
    }
    Ok(None)
}
