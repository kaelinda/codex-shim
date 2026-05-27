use std::env;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::time::Duration;

use error::{AppError, AppResult};
use tokio::fs::{self, OpenOptions};
use tokio::process::Command as TokioCommand;
use tokio::time::sleep;

mod catalog;
mod config;
mod embedded_shim;
mod error;
mod health;
mod models;
mod paths;

#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

async fn run() -> AppResult<()> {
    let parsed = Args::parse(env::args().skip(1).collect())?;
    match parsed.command {
        CommandSpec::Generate => generate(&parsed.settings, parsed.port).await,
        CommandSpec::List => list_models(&parsed.settings).await,
        CommandSpec::Start => {
            generate(&parsed.settings, parsed.port).await?;
            start_daemon(&parsed.settings, parsed.port).await
        }
        CommandSpec::Enable => {
            generate(&parsed.settings, parsed.port).await?;
            start_daemon(&parsed.settings, parsed.port).await?;
            install_codex_config(&parsed.settings, parsed.port, None).await
        }
        CommandSpec::Stop => stop_daemon().await,
        CommandSpec::Disable => {
            restore_codex_config().await?;
            stop_daemon().await
        }
        CommandSpec::Restart => {
            let _ = stop_daemon().await;
            generate(&parsed.settings, parsed.port).await?;
            start_daemon(&parsed.settings, parsed.port).await
        }
        CommandSpec::Status => status(parsed.port).await,
        CommandSpec::Configure => configure(&parsed.settings).await,
        CommandSpec::Serve => serve(parsed.settings, parsed.port).await,
        CommandSpec::ModelList => list_models(&parsed.settings).await,
        CommandSpec::ModelUse(slug) => {
            generate(&parsed.settings, parsed.port).await?;
            ensure_started(&parsed.settings, parsed.port).await?;
            install_codex_config(&parsed.settings, parsed.port, Some(slug)).await
        }
        CommandSpec::Codex(args) => {
            generate(&parsed.settings, parsed.port).await?;
            ensure_started(&parsed.settings, parsed.port).await?;
            exec_codex(&parsed.settings, parsed.port, args).await
        }
        CommandSpec::Help => {
            print_help();
            Ok(())
        }
    }
}

#[derive(Debug)]
struct Args {
    settings: PathBuf,
    port: u16,
    command: CommandSpec,
}

#[derive(Debug)]
enum CommandSpec {
    Generate,
    List,
    Start,
    Enable,
    Stop,
    Disable,
    Restart,
    Status,
    Configure,
    Serve,
    ModelList,
    ModelUse(String),
    Codex(Vec<String>),
    Help,
}

impl Args {
    fn parse(raw: Vec<String>) -> AppResult<Self> {
        let mut settings = paths::default_settings_path();
        let mut port = paths::DEFAULT_PORT;
        let mut idx = 0;
        while idx < raw.len() {
            match raw[idx].as_str() {
                "--settings" => {
                    idx += 1;
                    let value = raw
                        .get(idx)
                        .ok_or_else(|| AppError::msg("--settings needs a path"))?;
                    settings = expand_tilde(value);
                }
                "--port" => {
                    idx += 1;
                    let value = raw
                        .get(idx)
                        .ok_or_else(|| AppError::msg("--port needs a number"))?;
                    port = value
                        .parse::<u16>()
                        .map_err(|_| AppError::msg(format!("invalid --port value: {value}")))?;
                }
                "-h" | "--help" => {
                    return Ok(Self {
                        settings,
                        port,
                        command: CommandSpec::Help,
                    });
                }
                _ => break,
            }
            idx += 1;
        }

        let command = match raw.get(idx).map(String::as_str) {
            None => CommandSpec::Help,
            Some("generate") => CommandSpec::Generate,
            Some("list") => CommandSpec::List,
            Some("start") => CommandSpec::Start,
            Some("enable") => CommandSpec::Enable,
            Some("stop") => CommandSpec::Stop,
            Some("disable") => CommandSpec::Disable,
            Some("restart") => CommandSpec::Restart,
            Some("status") => CommandSpec::Status,
            Some("configure") => CommandSpec::Configure,
            Some("serve") => CommandSpec::Serve,
            Some("model") => match raw.get(idx + 1).map(String::as_str) {
                Some("list") => CommandSpec::ModelList,
                Some("use") => {
                    let slug = raw
                        .get(idx + 2)
                        .ok_or_else(|| AppError::msg("model use needs a model slug"))?;
                    CommandSpec::ModelUse(slug.to_string())
                }
                _ => return Err(AppError::msg("usage: codex-shim-cli model list|use <slug>")),
            },
            Some("codex") => {
                let mut args = raw.get(idx + 1..).unwrap_or_default().to_vec();
                if args.first().map(String::as_str) == Some("--") {
                    args.remove(0);
                }
                CommandSpec::Codex(args)
            }
            Some(other) => return Err(AppError::msg(format!("unknown command: {other}"))),
        };

        Ok(Self {
            settings,
            port,
            command,
        })
    }
}

fn print_help() {
    println!(
        "codex-shim-cli\n\n\
Usage:\n\
  codex-shim-cli [--settings PATH] [--port PORT] <command>\n\n\
Commands:\n\
  configure           Interactively add a model/API key to ~/.codex-shim/models.json\n\
  generate            Generate Codex catalog/config under ~/.codex-shim/cli\n\
  start               Start the Rust shim daemon on 127.0.0.1:8765\n\
  enable              Start daemon and install the managed ~/.codex/config.toml block\n\
  stop                Stop daemon\n\
  disable             Restore Codex config and stop daemon\n\
  restart             Restart daemon\n\
  status              Health check + model count\n\
  list                List configured models\n\
  model list          List configured models\n\
  model use <slug>    Select a model in ~/.codex/config.toml\n\
  codex -- <args...>  Run Codex CLI with shim config overrides\n"
    );
}

async fn generate(settings_path: &Path, port: u16) -> AppResult<()> {
    let models = load_models(settings_path).await?;
    let auth = config::read_codex_auth(&paths::codex_auth_path()).await?;
    let catalog_path = paths::catalog_path();
    let config_path = paths::generated_config_path();
    catalog::write_catalog(&models, &catalog_path, auth.passthrough_available).await?;
    catalog::write_generated_config(
        &models,
        &config_path,
        &catalog_path,
        port,
        auth.passthrough_available,
    )
    .await?;
    println!("Generated {} model entries:", models.len());
    println!("  catalog: {}", catalog_path.display());
    println!("  config:  {}", config_path.display());
    println!("No files under ~/.codex were modified.");
    Ok(())
}

async fn load_models(settings_path: &Path) -> AppResult<Vec<models::ShimModel>> {
    let file = models::read_file(settings_path).await?;
    Ok(models::model_rows(&file))
}

async fn list_models(settings_path: &Path) -> AppResult<()> {
    let file = models::read_file(settings_path).await?;
    let models = models::model_rows(&file);
    let auth = config::read_codex_auth(&paths::codex_auth_path()).await?;
    let mut rows: Vec<(String, String, String, String)> = Vec::new();
    if auth.passthrough_available {
        rows.push((
            "gpt-5.5".to_string(),
            "GPT-5.5".to_string(),
            "gpt-5.5".to_string(),
            "chatgpt".to_string(),
        ));
    }
    rows.extend(
        models
            .into_iter()
            .map(|model| (model.slug, model.display_name, model.model, model.provider)),
    );
    if rows.is_empty() {
        return Err(AppError::msg(
            "No models available. Run `codex-shim-cli configure` or `codex login`.",
        ));
    }
    let width = rows.iter().map(|row| row.0.len()).max().unwrap_or(0);
    for (slug, display, model, provider) in rows {
        println!("{slug:<width$}  {display}  ->  {model} ({provider})");
    }
    Ok(())
}

async fn start_daemon(settings_path: &Path, port: u16) -> AppResult<()> {
    let pid_path = paths::pid_path();
    if let Some(pid) = read_pid(&pid_path).await {
        if pid_running(pid) {
            if healthy(port).await {
                println!("Shim already running with pid {pid}.");
                return Ok(());
            }
            return Err(AppError::msg(format!(
                "Shim pid {pid} is running, but http://{}:{port}/health is not healthy. Run `codex-shim-cli stop` before changing ports.",
                paths::DEFAULT_HOST
            )));
        }
    }

    let runtime = paths::app_runtime_dir();
    fs::create_dir_all(&runtime).await?;
    let log_path = paths::log_path();
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .await?
        .into_std()
        .await;
    let stderr = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .await?
        .into_std()
        .await;
    let exe = env::current_exe()?;
    let mut command = StdCommand::new(exe);
    command
        .arg("--settings")
        .arg(settings_path)
        .arg("--port")
        .arg(port.to_string())
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    #[cfg(windows)]
    {
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let mut child = command.spawn()?;
    let pid = child.id();
    fs::write(&pid_path, pid.to_string()).await?;

    for _ in 0..50 {
        if healthy(port).await {
            println!(
                "Shim started on http://{}:{port} with pid {pid}.",
                paths::DEFAULT_HOST
            );
            println!("Log: {}", log_path.display());
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(AppError::msg(format!(
                "Shim exited during startup with status {status}. See {}.",
                log_path.display()
            )));
        }
        sleep(Duration::from_millis(100)).await;
    }
    Err(AppError::msg(format!(
        "Shim process started but health check timed out. See {}.",
        log_path.display()
    )))
}

async fn stop_daemon() -> AppResult<()> {
    let pid_path = paths::pid_path();
    let Some(pid) = read_pid(&pid_path).await else {
        println!("Shim is not running.");
        return Ok(());
    };
    if !pid_running(pid) {
        let _ = fs::remove_file(&pid_path).await;
        println!("Shim is not running.");
        return Ok(());
    }
    terminate_pid(pid).await?;
    for _ in 0..50 {
        if !pid_running(pid) {
            let _ = fs::remove_file(&pid_path).await;
            println!("Shim stopped.");
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }
    Err(AppError::msg(format!(
        "Shim pid {pid} did not exit after SIGTERM."
    )))
}

async fn status(port: u16) -> AppResult<()> {
    let pid = read_pid(&paths::pid_path()).await;
    let running = pid.map(pid_running).unwrap_or(false);
    let health = health::probe(port).await?;
    if running && health.ok {
        let models = health
            .models
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        println!(
            "Shim is running on http://{}:{port} with pid {} ({models} models).",
            paths::DEFAULT_HOST,
            pid.unwrap()
        );
        return Ok(());
    }
    if running {
        return Err(AppError::msg(format!(
            "Shim process {} exists but health check failed: {}",
            pid.unwrap(),
            health.error.unwrap_or_else(|| "unknown error".to_string())
        )));
    }
    Err(AppError::msg("Shim is stopped."))
}

async fn serve(settings_path: PathBuf, port: u16) -> AppResult<()> {
    let state = embedded_shim::EmbeddedShimState::default();
    let status = state.start(settings_path, port).await?;
    println!("{}", status.message);
    tokio::signal::ctrl_c().await?;
    let stopped = state.stop();
    println!("{}", stopped.message);
    Ok(())
}

async fn install_codex_config(
    settings_path: &Path,
    port: u16,
    requested: Option<String>,
) -> AppResult<()> {
    let models = load_models(settings_path).await?;
    let auth = config::read_codex_auth(&paths::codex_auth_path()).await?;
    let slug =
        resolve_model_slug(&models, auth.passthrough_available, requested.as_deref()).await?;
    config::install_codex_config(
        &paths::codex_config_path(),
        &models,
        auth.passthrough_available,
        &slug,
        port,
    )
    .await?;
    println!(
        "Installed shim config into {}.",
        paths::codex_config_path().display()
    );
    println!("Active Codex shim model: {slug}");
    Ok(())
}

async fn restore_codex_config() -> AppResult<()> {
    let restored = config::restore_codex_config(&paths::codex_config_path()).await?;
    if restored {
        println!(
            "Restored original {}.",
            paths::codex_config_path().display()
        );
    } else {
        println!(
            "Removed shim config from {}.",
            paths::codex_config_path().display()
        );
    }
    Ok(())
}

async fn resolve_model_slug(
    models: &[models::ShimModel],
    passthrough_available: bool,
    requested: Option<&str>,
) -> AppResult<String> {
    let Some(requested) = requested else {
        if let Some(current) = config::read_active_model(&paths::codex_config_path()).await? {
            return Ok(current);
        }
        return Ok(models::default_model_slug(models, passthrough_available));
    };
    if requested == "gpt-5.5" || requested == "openai-gpt-5-5" {
        if passthrough_available {
            return Ok("gpt-5.5".to_string());
        }
        return Err(AppError::msg(
            "gpt-5.5 passthrough requires `codex login` so ~/.codex/auth.json contains tokens.access_token.",
        ));
    }
    if let Some(model) = models.iter().find(|model| model.slug == requested) {
        return Ok(model.slug.clone());
    }
    let by_model: Vec<&models::ShimModel> = models
        .iter()
        .filter(|model| model.model == requested)
        .collect();
    if by_model.len() == 1 {
        return Ok(by_model[0].slug.clone());
    }
    let matches: Vec<String> = models
        .iter()
        .filter(|model| {
            model
                .display_name
                .to_lowercase()
                .contains(&requested.to_lowercase())
        })
        .map(|model| model.slug.clone())
        .collect();
    if matches.len() == 1 {
        return Ok(matches[0].clone());
    }
    if !matches.is_empty() {
        return Err(AppError::msg(format!(
            "Ambiguous model {requested:?}. Matches: {}",
            matches.join(", ")
        )));
    }
    Err(AppError::msg(format!(
        "Unknown shim model {requested:?}. Run: codex-shim-cli model list"
    )))
}

async fn ensure_started(settings_path: &Path, port: u16) -> AppResult<()> {
    if healthy(port).await {
        return Ok(());
    }
    start_daemon(settings_path, port).await
}

async fn exec_codex(settings_path: &Path, port: u16, args: Vec<String>) -> AppResult<()> {
    let models = load_models(settings_path).await?;
    let auth = config::read_codex_auth(&paths::codex_auth_path()).await?;
    let default_slug = models::default_model_slug(&models, auth.passthrough_available);
    let catalog = catalog::toml_escape(&paths::catalog_path().display().to_string());
    let overrides = [
        format!("model=\"{}\"", catalog::toml_escape(&default_slug)),
        "model_provider=\"codex_shim\"".to_string(),
        format!("model_catalog_json=\"{catalog}\""),
        "model_providers.codex_shim.name=\"Codex Shim\"".to_string(),
        format!(
            "model_providers.codex_shim.base_url=\"http://{}:{port}/v1\"",
            paths::DEFAULT_HOST
        ),
        "model_providers.codex_shim.wire_api=\"responses\"".to_string(),
        "model_providers.codex_shim.experimental_bearer_token=\"dummy\"".to_string(),
        "model_providers.codex_shim.request_max_retries=3".to_string(),
        "model_providers.codex_shim.stream_max_retries=3".to_string(),
        "model_providers.codex_shim.stream_idle_timeout_ms=600000".to_string(),
    ];

    let mut command = TokioCommand::new("codex");
    for value in overrides {
        command.arg("-c").arg(value);
    }
    command.args(args);
    let status = command.status().await?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::msg(format!("codex exited with status {status}")))
    }
}

async fn configure(settings_path: &Path) -> AppResult<()> {
    if !io::stdin().is_terminal() {
        return Err(AppError::msg("configure requires an interactive terminal"));
    }
    println!("Configure a BYOK model in {}", settings_path.display());
    println!("Press Enter to accept defaults in brackets.");

    let default_provider = "openai";
    let provider = prompt_default("provider", default_provider)?;
    let default_base_url = match provider.as_str() {
        "anthropic" => "https://api.anthropic.com/v1",
        "deepseek" => "https://api.deepseek.com",
        "dashscope" => "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "moonshot" => "https://api.moonshot.cn/v1",
        "volcengine" => "https://ark.cn-beijing.volces.com/api/v3",
        _ => "https://api.openai.com/v1",
    };
    let base_url = prompt_default("base_url", default_base_url)?;
    let model = prompt_required("model")?;
    let display_name = prompt_default("display_name", &model)?;
    let api_key = prompt_secret("api_key")?;
    let no_image_support = prompt_default("no_image_support true/false", "false")?
        .parse::<bool>()
        .unwrap_or(false);

    let mut file = models::read_file(settings_path).await?;
    file.models.push(models::ModelRow {
        model,
        provider,
        base_url,
        api_key,
        display_name: Some(display_name),
        no_image_support,
        ..Default::default()
    });
    for row in &file.models {
        models::validate(row)?;
    }
    models::write_file(settings_path, &file).await?;
    println!("Saved model config to {}.", settings_path.display());
    println!("API keys stay in this settings file and are not copied into generated Codex catalog/config.");
    Ok(())
}

fn prompt_required(label: &str) -> AppResult<String> {
    loop {
        let value = prompt(label)?;
        if !value.trim().is_empty() {
            return Ok(value.trim().to_string());
        }
        eprintln!("{label} is required.");
    }
}

fn prompt_default(label: &str, default: &str) -> AppResult<String> {
    let value = prompt(&format!("{label} [{default}]"))?;
    if value.trim().is_empty() {
        Ok(default.to_string())
    } else {
        Ok(value.trim().to_string())
    }
}

fn prompt_secret(label: &str) -> AppResult<String> {
    #[cfg(unix)]
    if io::stdin().is_terminal() {
        print!("{label}: ");
        io::stdout().flush()?;
        let _ = StdCommand::new("stty").arg("-echo").status();
        let mut value = String::new();
        let read = io::stdin().read_line(&mut value);
        let _ = StdCommand::new("stty").arg("echo").status();
        println!();
        read?;
        return Ok(value.trim_end_matches(['\r', '\n']).to_string());
    }
    let value = prompt(label)?;
    Ok(value.trim().to_string())
}

fn prompt(label: &str) -> AppResult<String> {
    print!("{label}: ");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(value.trim_end_matches(['\r', '\n']).to_string())
}

async fn healthy(port: u16) -> bool {
    health::probe(port)
        .await
        .map(|snapshot| snapshot.ok)
        .unwrap_or(false)
}

async fn read_pid(path: &Path) -> Option<u32> {
    fs::read_to_string(path)
        .await
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
}

fn pid_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        StdCommand::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        StdCommand::new("cmd")
            .args(["/C", "tasklist", "/FI", &format!("PID eq {pid}")])
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
}

async fn terminate_pid(pid: u32) -> AppResult<()> {
    #[cfg(unix)]
    {
        let status = TokioCommand::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status()
            .await?;
        if status.success() {
            Ok(())
        } else {
            Err(AppError::msg(format!("failed to terminate pid {pid}")))
        }
    }
    #[cfg(windows)]
    {
        let status = TokioCommand::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T"])
            .status()
            .await?;
        if status.success() {
            Ok(())
        } else {
            Err(AppError::msg(format!("failed to terminate pid {pid}")))
        }
    }
}

fn expand_tilde(value: &str) -> PathBuf {
    if value == "~" {
        return paths::home_dir();
    }
    if let Some(stripped) = value.strip_prefix("~/") {
        return paths::home_dir().join(stripped);
    }
    PathBuf::from(value)
}
