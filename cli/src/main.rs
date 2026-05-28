use std::env;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use error::{AppError, AppResult};
use reqwest::header::{HeaderName, HeaderValue, CONTENT_TYPE};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::AsyncReadExt;
use tokio::process::Command as TokioCommand;
use tokio::time::sleep;

mod catalog;
mod config;
mod embedded_shim;
mod error;
mod health;
mod models;
mod paths;
mod updater;

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
        CommandSpec::ExportConfig { path, without_keys } => {
            export_config(&parsed.settings, &path, without_keys).await
        }
        CommandSpec::ImportConfig(path) => import_config(&parsed.settings, &path).await,
        CommandSpec::Test(target) => test_model(&parsed.settings, &target).await,
        CommandSpec::Version => {
            print_version();
            Ok(())
        }
        CommandSpec::Update { install } => update_cli(install).await,
        CommandSpec::PatchApp => patch_codex_app().await,
        CommandSpec::RestoreApp => restore_codex_app_bundle().await,
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
    ExportConfig { path: PathBuf, without_keys: bool },
    ImportConfig(PathBuf),
    Test(String),
    Version,
    Update { install: bool },
    PatchApp,
    RestoreApp,
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
            Some("export") => parse_export_command(raw.get(idx + 1..).unwrap_or_default())?,
            Some("import") => parse_import_command(raw.get(idx + 1..).unwrap_or_default())?,
            Some("version") | Some("--version") | Some("-V") => CommandSpec::Version,
            Some("update") | Some("upgrade") => {
                parse_update_command(raw.get(idx + 1..).unwrap_or_default())?
            }
            Some("patch-app") => CommandSpec::PatchApp,
            Some("restore-app") | Some("unpatch-app") => CommandSpec::RestoreApp,
            Some("config") => match raw.get(idx + 1).map(String::as_str) {
                Some("export") => parse_export_command(raw.get(idx + 2..).unwrap_or_default())?,
                Some("import") => parse_import_command(raw.get(idx + 2..).unwrap_or_default())?,
                _ => {
                    return Err(AppError::msg(
                        "usage: codex-shim-cli config export|import <path>",
                    ));
                }
            },
            Some("test") => {
                let target = raw.get(idx + 1).ok_or_else(|| {
                    AppError::msg("usage: codex-shim-cli test <slug|provider|model>")
                })?;
                CommandSpec::Test(target.to_string())
            }
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

fn parse_export_command(args: &[String]) -> AppResult<CommandSpec> {
    let mut without_keys = false;
    let mut path: Option<PathBuf> = None;
    let mut idx = 0;
    while idx < args.len() {
        match args[idx].as_str() {
            "--without-keys" => without_keys = true,
            "--with-keys" => without_keys = false,
            "-h" | "--help" => {
                return Err(AppError::msg(
                    "usage: codex-shim-cli export [--without-keys] <path>",
                ));
            }
            value if value.starts_with('-') => {
                return Err(AppError::msg(format!("unknown export option: {value}")));
            }
            value => {
                if path.is_some() {
                    return Err(AppError::msg(
                        "usage: codex-shim-cli export [--without-keys] <path>",
                    ));
                }
                path = Some(expand_tilde(value));
            }
        }
        idx += 1;
    }
    let path =
        path.ok_or_else(|| AppError::msg("usage: codex-shim-cli export [--without-keys] <path>"))?;
    Ok(CommandSpec::ExportConfig { path, without_keys })
}

fn parse_import_command(args: &[String]) -> AppResult<CommandSpec> {
    if args.len() != 1 {
        return Err(AppError::msg("usage: codex-shim-cli import <path>"));
    }
    Ok(CommandSpec::ImportConfig(expand_tilde(&args[0])))
}

fn parse_update_command(args: &[String]) -> AppResult<CommandSpec> {
    let mut install = false;
    for arg in args {
        match arg.as_str() {
            "--install" | "-i" => install = true,
            "--check" => install = false,
            "-h" | "--help" => {
                return Err(AppError::msg("usage: codex-shim-cli update [--install]"));
            }
            value => return Err(AppError::msg(format!("unknown update option: {value}"))),
        }
    }
    Ok(CommandSpec::Update { install })
}

fn print_help() {
    println!(
        "codex-shim-cli\n\n\
用法:\n\
  codex-shim-cli [--settings PATH] [--port PORT] <command>\n\n\
命令:\n\
  configure           交互式添加模型和 API Key 到 ~/.codex-shim/models.json\n\
  generate            生成 Codex catalog/config 到 ~/.codex-shim/cli\n\
  start               启动 127.0.0.1:8765 上的 Rust shim 守护进程\n\
  enable              启动守护进程并写入 ~/.codex/config.toml 托管配置\n\
  stop                停止守护进程\n\
  disable             恢复 Codex 配置并停止守护进程\n\
  restart             重启守护进程\n\
  status              健康检查和模型数量\n\
  list                列出已配置模型\n\
  export <path>       导出 models.json，便于复制到其他设备\n\
  import <path>       导入 models.json，并自动备份当前配置\n\
  config export <path>  同 export <path>\n\
  config import <path>  同 import <path>\n\
  test <name>         测试指定 provider、slug 或上游模型是否可用\n\
  version             显示当前 CLI 版本\n\
  update [--install]  检查 GitHub Releases 更新，--install 会重新安装 CLI\n\
  patch-app           给 macOS Codex Desktop 模型选择器打补丁\n\
  restore-app         撤销补丁，恢复原始 Codex Desktop app.asar\n\
  model list          列出已配置模型\n\
  model use <slug>    在 ~/.codex/config.toml 中选择模型\n\
  codex -- <args...>  使用 shim 配置覆盖项运行 Codex CLI\n"
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
    println!("已生成 {} 个模型条目：", models.len());
    println!("  catalog: {}", catalog_path.display());
    println!("  config:  {}", config_path.display());
    println!("未修改 ~/.codex 下的文件。");
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
            "没有可用模型。请运行 `codex-shim-cli configure` 或 `codex login`。",
        ));
    }
    let width = rows.iter().map(|row| row.0.len()).max().unwrap_or(0);
    for (slug, display, model, provider) in rows {
        println!("{slug:<width$}  {display}  ->  {model} ({provider})");
    }
    Ok(())
}

async fn export_config(
    settings_path: &Path,
    export_path: &Path,
    without_keys: bool,
) -> AppResult<()> {
    let mut file = models::read_file(settings_path).await?;
    validate_models_file(&file)?;
    if without_keys {
        for row in &mut file.models {
            row.api_key.clear();
        }
    }
    if let Some(parent) = export_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).await?;
        }
    }
    models::write_file(export_path, &file).await?;
    println!("已导出 provider 配置到：{}", export_path.display());
    if without_keys {
        println!("导出文件已移除 API Key。导入到新设备后需要重新填写 api_key。");
    } else {
        println!("注意：导出文件包含 API Key，请只保存在可信位置。");
    }
    println!("模型数量：{}", file.models.len());
    Ok(())
}

async fn import_config(settings_path: &Path, import_path: &Path) -> AppResult<()> {
    let file = models::read_file(import_path).await?;
    validate_models_file(&file)?;
    if file.models.is_empty() {
        return Err(AppError::msg("导入文件中没有任何 provider 配置。"));
    }

    if settings_path.exists() {
        let backup_path = backup_path_for(settings_path)?;
        if let Some(parent) = backup_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).await?;
            }
        }
        fs::copy(settings_path, &backup_path).await?;
        println!("已备份当前配置到：{}", backup_path.display());
    }

    models::write_file(settings_path, &file).await?;
    println!("已导入 provider 配置到：{}", settings_path.display());
    println!("模型数量：{}", file.models.len());
    println!("可运行 `codex-shim-cli list` 查看，或 `codex-shim-cli test <provider>` 测试。");
    Ok(())
}

fn validate_models_file(file: &models::ModelsFile) -> AppResult<()> {
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

async fn test_model(settings_path: &Path, target: &str) -> AppResult<()> {
    let file = models::read_file(settings_path).await?;
    let routes = resolve_test_targets(&test_routes(&file), target)?;
    let total = routes.len();
    if total > 1 {
        println!("匹配到 {total} 个模型，开始逐条测试。");
        println!();
    }

    let mut failures = Vec::new();
    for route in routes {
        if total > 1 {
            println!("== {} ==", route.slug);
        }
        if let Err(err) = test_one_model(&route).await {
            failures.push(format!("{}: {err}", route.slug));
        }
        if total > 1 {
            println!();
        }
    }

    if failures.is_empty() {
        if total > 1 {
            println!("全部测试通过。");
        }
        return Ok(());
    }
    Err(AppError::msg(format!(
        "{} 个测试失败：\n{}",
        failures.len(),
        failures.join("\n")
    )))
}

fn print_version() {
    println!("codex-shim-cli {}", env!("CARGO_PKG_VERSION"));
}

async fn update_cli(install: bool) -> AppResult<()> {
    let info = match updater::check_latest_release().await {
        Ok(info) => info,
        Err(err) if install => {
            eprintln!("检查 GitHub Releases 失败：{err}");
            eprintln!(
                "继续使用默认分支 {} 运行安装器。",
                updater::default_update_ref()
            );
            updater::fallback_update_info(None)
        }
        Err(err) => return Err(err),
    };

    println!("当前版本：{}", info.current_version);
    if info.latest_version.is_empty() {
        println!("最新版本：无法从 GitHub Releases 获取");
    } else {
        println!("最新版本：{} ({})", info.latest_version, info.latest_tag);
    }
    println!("发布页：{}", info.release_url);

    if info.update_available {
        println!("发现新版本。");
    } else if !info.latest_version.is_empty() {
        println!("当前已是最新版本。");
    }

    if !info.assets.is_empty() {
        println!("可下载文件：");
        for asset in &info.assets {
            println!("  - {}: {}", asset.name, asset.download_url);
        }
    }

    println!("安装命令：");
    println!("  {}", info.install_command);

    if !install {
        println!("如需立即更新 CLI，请运行：codex-shim-cli update --install");
        return Ok(());
    }

    println!("开始更新 CLI...");
    let output = updater::install_cli_update(Some(&info.install_ref)).await?;
    if !output.stdout.trim().is_empty() {
        println!("{}", output.stdout.trim_end());
    }
    if !output.stderr.trim().is_empty() {
        eprintln!("{}", output.stderr.trim_end());
    }
    if output.ok {
        println!("CLI 更新完成。");
        return Ok(());
    }
    Err(AppError::msg(format!(
        "CLI 更新失败，退出状态：{}",
        output
            .status
            .map(|status| status.to_string())
            .unwrap_or_else(|| "-".to_string())
    )))
}

const CODEX_APP_BUNDLE: &str = "/Applications/Codex.app";
const CODEX_APP_ASAR: &str = "/Applications/Codex.app/Contents/Resources/app.asar";
const CODEX_INFO_PLIST: &str = "/Applications/Codex.app/Contents/Info.plist";
const ASAR_BACKUP_NAME: &str = "app.asar.before-codex-shim-model-picker-patch";
const INFO_PLIST_BACKUP_NAME: &str = "Info.plist.before-codex-shim-model-picker-patch";
const PICKER_NEEDLE: &str = "let u=c.useHiddenModels&&o!==`amazonBedrock`,d;";
const PICKER_REPLACEMENT: &str = "let u=!1,d;";

async fn patch_codex_app() -> AppResult<()> {
    ensure_macos_app_patch_supported("patch-app")?;

    let app_asar = PathBuf::from(CODEX_APP_ASAR);
    let info_plist = PathBuf::from(CODEX_INFO_PLIST);
    let runtime = paths::app_runtime_dir();
    let backup = import_known_backup(ASAR_BACKUP_NAME).await?;
    let plist_backup = import_known_backup(INFO_PLIST_BACKUP_NAME).await?;
    let workdir = runtime.join("app-asar-work");

    if !app_asar.exists() {
        return Err(AppError::msg(format!(
            "未找到 Codex Desktop：{}",
            app_asar.display()
        )));
    }
    if which::which("npx").is_err() {
        return Err(AppError::msg(
            "patch-app 需要 npx 来解包 Electron ASAR，请先安装 Node.js/npm。",
        ));
    }

    fs::create_dir_all(&runtime).await?;
    let _ = quit_codex_app().await;
    if workdir.exists() {
        fs::remove_dir_all(&workdir).await?;
    }
    fs::create_dir_all(&workdir).await?;

    run_command(
        "npx",
        &[
            "--yes",
            "asar",
            "extract",
            &app_asar.display().to_string(),
            &workdir.display().to_string(),
        ],
    )
    .await?;

    let Some(bundle_file) =
        find_model_queries_bundle(&workdir, PICKER_NEEDLE, PICKER_REPLACEMENT).await?
    else {
        return Err(AppError::msg(
            "未找到 Codex Desktop 模型选择器过滤逻辑，当前 Codex 版本可能已变更。",
        ));
    };

    let text = fs::read_to_string(&bundle_file).await.unwrap_or_default();
    if text.contains(PICKER_REPLACEMENT) {
        if !backup.exists() {
            return Err(AppError::msg(
                "Codex Desktop 模型选择器补丁已存在，但未找到原始 app.asar 备份，无法保证可撤销。请用手动备份恢复 Codex.app。",
            ));
        }
        println!("Codex Desktop 模型选择器补丁已存在，无需重复修改。");
    } else if text.contains(PICKER_NEEDLE) {
        if !backup.exists() {
            fs::copy(&app_asar, &backup).await?;
            println!("已备份原始 app.asar：{}", backup.display());
        } else {
            println!("已存在原始备份：{}", backup.display());
        }
        if info_plist.exists() && !plist_backup.exists() {
            fs::copy(&info_plist, &plist_backup).await?;
            println!("已备份原始 Info.plist：{}", plist_backup.display());
        }

        let current_hash = file_sha256(&app_asar).await?;
        let versioned_backup = runtime.join(format!(
            "{ASAR_BACKUP_NAME}.{}",
            current_hash.chars().take(12).collect::<String>()
        ));
        if !versioned_backup.exists() {
            fs::copy(&app_asar, &versioned_backup).await?;
            println!("已备份当前 app.asar：{}", versioned_backup.display());
        }

        fs::write(
            &bundle_file,
            text.replace(PICKER_NEEDLE, PICKER_REPLACEMENT),
        )
        .await?;
        run_command(
            "npx",
            &[
                "--yes",
                "asar",
                "pack",
                &workdir.display().to_string(),
                &app_asar.display().to_string(),
            ],
        )
        .await?;
        println!("已应用 Codex Desktop 模型选择器补丁。");
        resign_codex_app().await?;
    } else {
        return Err(AppError::msg(
            "未找到可替换的模型选择器过滤逻辑，当前 Codex 版本可能已变更。",
        ));
    }

    println!("如需撤销补丁，请运行：codex-shim-cli restore-app");
    Ok(())
}

async fn restore_codex_app_bundle() -> AppResult<()> {
    ensure_macos_app_patch_supported("restore-app")?;

    let app_asar = PathBuf::from(CODEX_APP_ASAR);
    let info_plist = PathBuf::from(CODEX_INFO_PLIST);
    let Some(backup) = find_known_backup(ASAR_BACKUP_NAME) else {
        let expected = paths::app_runtime_dir().join(ASAR_BACKUP_NAME);
        println!("未找到 app.asar 备份：{}", expected.display());
        println!("没有需要恢复的 Codex Desktop 补丁。");
        return Ok(());
    };
    let plist_backup = find_known_backup(INFO_PLIST_BACKUP_NAME);
    if !backup.exists() {
        println!("未找到 app.asar 备份：{}", backup.display());
        println!("没有需要恢复的 Codex Desktop 补丁。");
        return Ok(());
    }
    if !app_asar.exists() {
        return Err(AppError::msg(format!(
            "未找到 Codex Desktop：{}",
            app_asar.display()
        )));
    }

    let _ = quit_codex_app().await;
    fs::copy(&backup, &app_asar).await?;
    println!(
        "已从备份恢复原始 Codex Desktop app.asar：{}",
        backup.display()
    );
    if let Some(plist_backup) = plist_backup.filter(|_| info_plist.exists()) {
        fs::copy(&plist_backup, &info_plist).await?;
        println!(
            "已从备份恢复原始 Codex Desktop Info.plist：{}",
            plist_backup.display()
        );
    }
    resign_codex_app().await?;
    println!("Codex Desktop 补丁已撤销。");
    Ok(())
}

async fn import_known_backup(filename: &str) -> AppResult<PathBuf> {
    let primary = paths::app_runtime_dir().join(filename);
    if primary.exists() {
        return Ok(primary);
    }
    if let Some(existing) = known_backup_paths(filename)
        .into_iter()
        .skip(1)
        .find(|path| path.exists())
    {
        if let Some(parent) = primary.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(&existing, &primary).await?;
        println!(
            "已复用现有备份：{} -> {}",
            existing.display(),
            primary.display()
        );
    }
    Ok(primary)
}

fn find_known_backup(filename: &str) -> Option<PathBuf> {
    known_backup_paths(filename)
        .into_iter()
        .find(|path| path.exists())
}

fn known_backup_paths(filename: &str) -> Vec<PathBuf> {
    vec![
        paths::app_runtime_dir().join(filename),
        paths::home_dir()
            .join(".codex-shim")
            .join("app")
            .join(filename),
    ]
}

fn ensure_macos_app_patch_supported(command: &str) -> AppResult<()> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        Err(AppError::msg(format!(
            "{command} 仅支持 macOS，因为它针对 /Applications/Codex.app。"
        )))
    }
}

async fn quit_codex_app() -> AppResult<()> {
    let script = r#"tell application "Codex" to if it is running then quit"#;
    let _ = run_quiet("osascript", &["-e", script]).await;
    sleep(Duration::from_millis(1000)).await;
    Ok(())
}

async fn resign_codex_app() -> AppResult<()> {
    run_command(
        "codesign",
        &["--force", "--deep", "--sign", "-", CODEX_APP_BUNDLE],
    )
    .await?;
    println!("已重新签名 Codex.app。");
    Ok(())
}

async fn run_quiet(program: &str, args: &[&str]) -> AppResult<()> {
    let _ = TokioCommand::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?;
    Ok(())
}

async fn run_command(program: &str, args: &[&str]) -> AppResult<()> {
    let output = TokioCommand::new(program).args(args).output().await?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if !stderr.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };
    Err(AppError::msg(format!(
        "命令执行失败：{} {}\n{}",
        program,
        args.join(" "),
        detail
    )))
}

async fn file_sha256(path: &Path) -> AppResult<String> {
    let mut file = File::open(path).await?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut hasher = Sha256::new();
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
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            let priority = if name.starts_with("model-queries-") {
                0
            } else {
                1
            };
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

#[derive(Debug, Clone)]
struct TestRoute {
    slug: String,
    model: String,
    display_name: String,
    provider: String,
    base_url: String,
    api_key: String,
    max_output_tokens: Option<i64>,
    extra_headers: Option<serde_json::Map<String, Value>>,
}

fn test_routes(file: &models::ModelsFile) -> Vec<TestRoute> {
    let rows = models::model_rows(file);
    rows.into_iter()
        .filter_map(|model| {
            let row = file.models.get(model.index)?;
            Some(TestRoute {
                slug: model.slug,
                model: model.model,
                display_name: model.display_name,
                provider: model.provider,
                base_url: row.base_url.trim().trim_end_matches('/').to_string(),
                api_key: row.api_key.clone(),
                max_output_tokens: row.max_output_tokens,
                extra_headers: row.extra_headers.clone(),
            })
        })
        .collect()
}

async fn test_one_model(route: &TestRoute) -> AppResult<()> {
    let endpoint = if route.provider == "anthropic" {
        "/messages"
    } else {
        "/chat/completions"
    };
    let url = join_upstream_url(&route.base_url, endpoint);

    println!("正在测试上游 provider：{}", route.provider);
    println!("  slug:  {}", route.slug);
    println!("  model: {}", route.model);
    println!("  url:   {url}");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    let mut request = client.post(&url).header(CONTENT_TYPE, "application/json");
    let body = if route.provider == "anthropic" {
        request = request.header("anthropic-version", "2023-06-01");
        if !route.api_key.is_empty() {
            request = request.header("x-api-key", &route.api_key);
            request = request.bearer_auth(&route.api_key);
        }
        json!({
            "model": route.model,
            "max_tokens": route.max_output_tokens.filter(|value| *value > 0).unwrap_or(64).min(256),
            "messages": [{"role": "user", "content": "ping"}],
            "stream": false
        })
    } else {
        if !route.api_key.is_empty() {
            request = request.bearer_auth(&route.api_key);
        }
        json!({
            "model": route.model,
            "messages": [{"role": "user", "content": "ping"}],
            "stream": false
        })
    };
    request = apply_extra_headers(request, route.extra_headers.as_ref())?;

    let response = request.json(&body).send().await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(AppError::msg(format!(
            "测试失败：上游返回 {status}\n{}",
            truncate_text(&text, 1200)
        )));
    }

    println!("测试成功：上游返回 {status}");
    if let Ok(payload) = serde_json::from_str::<Value>(&text) {
        if let Some(preview) = response_preview(&payload, &route.provider) {
            println!("  response: {}", truncate_text(&preview, 300));
        }
    }
    Ok(())
}

fn resolve_test_targets(models: &[TestRoute], target: &str) -> AppResult<Vec<TestRoute>> {
    let target = target.trim();
    if target.is_empty() {
        return Err(AppError::msg(
            "usage: codex-shim-cli test <slug|provider|model>",
        ));
    }
    if let Some(model) = models.iter().find(|model| model.slug == target) {
        return Ok(vec![model.clone()]);
    }
    let provider_matches = models
        .iter()
        .filter(|model| model.provider == target)
        .cloned()
        .collect::<Vec<_>>();
    if !provider_matches.is_empty() {
        return Ok(provider_matches);
    }
    if let Some(model) = unique_match(
        models
            .iter()
            .filter(|model| model.model == target)
            .cloned()
            .collect(),
        target,
        "model",
    )? {
        return Ok(vec![model]);
    }
    let lower = target.to_lowercase();
    if let Some(model) = unique_match(
        models
            .iter()
            .filter(|model| model.display_name.to_lowercase().contains(&lower))
            .cloned()
            .collect(),
        target,
        "display_name",
    )? {
        return Ok(vec![model]);
    }
    Err(AppError::msg(format!(
        "未找到可测试的目标 {target:?}。请运行：codex-shim-cli list"
    )))
}

fn unique_match(matches: Vec<TestRoute>, target: &str, kind: &str) -> AppResult<Option<TestRoute>> {
    if matches.is_empty() {
        return Ok(None);
    }
    if matches.len() == 1 {
        return Ok(matches.into_iter().next());
    }
    let slugs = matches
        .iter()
        .map(|model| model.slug.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(AppError::msg(format!(
        "{kind} {target:?} 匹配到多个模型，请改用具体 slug：{slugs}"
    )))
}

fn apply_extra_headers(
    mut request: reqwest::RequestBuilder,
    headers: Option<&serde_json::Map<String, Value>>,
) -> AppResult<reqwest::RequestBuilder> {
    for (key, value) in headers.into_iter().flatten() {
        let rendered = value
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| value.to_string());
        let name = HeaderName::from_bytes(key.as_bytes()).map_err(|err| {
            AppError::msg(format!("extra_headers 中的 header 名称无效：{key}: {err}"))
        })?;
        let header_value = HeaderValue::from_str(&rendered).map_err(|err| {
            AppError::msg(format!("extra_headers 中的 header 值无效：{key}: {err}"))
        })?;
        request = request.header(name, header_value);
    }
    Ok(request)
}

fn response_preview(payload: &Value, provider: &str) -> Option<String> {
    if provider == "anthropic" {
        return payload
            .get("content")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string);
    }
    payload
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out = value.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

fn join_upstream_url(base_url: &str, endpoint: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1") {
        format!("{base}{endpoint}")
    } else if endpoint == "/messages" {
        format!("{base}/v1/messages")
    } else {
        format!("{base}/v1{endpoint}")
    }
}

async fn start_daemon(settings_path: &Path, port: u16) -> AppResult<()> {
    let pid_path = paths::pid_path();
    if let Some(pid) = read_pid(&pid_path).await {
        if pid_running(pid) {
            if healthy(port).await {
                println!("Shim 已在运行，pid 为 {pid}。");
                return Ok(());
            }
            return Err(AppError::msg(format!(
                "Shim pid {pid} 正在运行，但 http://{}:{port}/health 不健康。切换端口前请先运行 `codex-shim-cli stop`。",
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
                "Shim 已启动：http://{}:{port}，pid 为 {pid}。",
                paths::DEFAULT_HOST
            );
            println!("日志文件：{}", log_path.display());
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(AppError::msg(format!(
                "Shim 启动过程中退出，状态为 {status}。请查看 {}。",
                log_path.display()
            )));
        }
        sleep(Duration::from_millis(100)).await;
    }
    Err(AppError::msg(format!(
        "Shim 进程已启动，但健康检查超时。请查看 {}。",
        log_path.display()
    )))
}

async fn stop_daemon() -> AppResult<()> {
    let pid_path = paths::pid_path();
    let Some(pid) = read_pid(&pid_path).await else {
        println!("Shim 未运行。");
        return Ok(());
    };
    if !pid_running(pid) {
        let _ = fs::remove_file(&pid_path).await;
        println!("Shim 未运行。");
        return Ok(());
    }
    terminate_pid(pid).await?;
    for _ in 0..50 {
        if !pid_running(pid) {
            let _ = fs::remove_file(&pid_path).await;
            println!("Shim 已停止。");
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }
    Err(AppError::msg(format!(
        "Shim pid {pid} 收到 SIGTERM 后仍未退出。"
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
            "Shim 正在运行：http://{}:{port}，pid 为 {}（{models} 个模型）。",
            paths::DEFAULT_HOST,
            pid.unwrap()
        );
        return Ok(());
    }
    if running {
        return Err(AppError::msg(format!(
            "Shim 进程 {} 存在，但健康检查失败：{}",
            pid.unwrap(),
            health.error.unwrap_or_else(|| "unknown error".to_string())
        )));
    }
    Err(AppError::msg("Shim 已停止。"))
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
        "已写入 shim 配置到 {}。",
        paths::codex_config_path().display()
    );
    println!("当前 Codex shim 模型：{slug}");
    Ok(())
}

async fn restore_codex_config() -> AppResult<()> {
    let restored = config::restore_codex_config(&paths::codex_config_path()).await?;
    if restored {
        println!("已恢复原始配置 {}。", paths::codex_config_path().display());
    } else {
        println!(
            "已从 {} 移除 shim 配置。",
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
            "gpt-5.5 passthrough 需要先运行 `codex login`，确保 ~/.codex/auth.json 包含 tokens.access_token。",
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
            "模型 {requested:?} 不唯一。匹配项：{}",
            matches.join(", ")
        )));
    }
    Err(AppError::msg(format!(
        "未知 shim 模型 {requested:?}。请运行：codex-shim-cli model list"
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
        Err(AppError::msg(format!("codex 退出状态：{status}")))
    }
}

async fn configure(settings_path: &Path) -> AppResult<()> {
    if !io::stdin().is_terminal() {
        return Err(AppError::msg("configure 需要在交互式终端中运行"));
    }
    println!("正在配置 BYOK 模型：{}", settings_path.display());
    println!("方括号内是默认值，直接回车即可使用默认值。");
    println!();
    print_provider_menu();

    let provider = prompt_provider()?;
    let default_base_url = match provider.as_str() {
        "anthropic" => "https://api.anthropic.com/v1",
        "deepseek" => "https://api.deepseek.com",
        "dashscope" => "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "moonshot" => "https://api.moonshot.cn/v1",
        "volcengine" => "https://ark.cn-beijing.volces.com/api/v3",
        _ => "https://api.openai.com/v1",
    };
    let base_url = prompt_default("base_url，上游 API 根地址", default_base_url)?;
    let model = prompt_required("model，上游模型名称")?;
    let display_name = prompt_default("display_name，Codex 中展示的名称", &model)?;
    let api_key = prompt_secret("api_key，上游 API Key")?;
    let no_image_support = prompt_default("该模型是否不支持图片？true/false", "false")?
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
    println!("模型配置已保存到：{}", settings_path.display());
    println!("API Key 只保存在该配置文件中，不会写入生成的 Codex catalog/config。");
    Ok(())
}

fn print_provider_menu() {
    println!(
        "请选择 provider：\n\
  1) openai      https://api.openai.com/v1\n\
  2) anthropic  https://api.anthropic.com/v1\n\
  3) deepseek   https://api.deepseek.com\n\
  4) moonshot   https://api.moonshot.cn/v1\n\
  5) dashscope  https://dashscope.aliyuncs.com/compatible-mode/v1\n\
  6) volcengine https://ark.cn-beijing.volces.com/api/v3\n\
  7) custom     兼容 OpenAI chat-completions 的自定义网关"
    );
}

fn prompt_provider() -> AppResult<String> {
    let value = prompt_default("provider 名称或编号", "1")?;
    let provider = match value.trim() {
        "1" | "openai" => "openai",
        "2" | "anthropic" => "anthropic",
        "3" | "deepseek" => "deepseek",
        "4" | "moonshot" => "moonshot",
        "5" | "dashscope" => "dashscope",
        "6" | "volcengine" => "volcengine",
        "7" | "custom" => {
            return prompt_default(
                "自定义 provider 名称，list 会显示这个名字",
                "generic-chat-completion-api",
            );
        }
        other => other,
    };
    Ok(provider.to_string())
}

fn prompt_required(label: &str) -> AppResult<String> {
    loop {
        let value = prompt(label)?;
        if !value.trim().is_empty() {
            return Ok(value.trim().to_string());
        }
        eprintln!("{label} 必填。");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_route(slug: &str, provider: &str, model: &str) -> TestRoute {
        TestRoute {
            slug: slug.to_string(),
            model: model.to_string(),
            display_name: model.to_string(),
            provider: provider.to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            api_key: "test-key".to_string(),
            max_output_tokens: None,
            extra_headers: None,
        }
    }

    #[test]
    fn test_target_provider_can_match_multiple_models() {
        let models = vec![
            test_route("model-a", "new-api", "model-a"),
            test_route("model-b", "new-api", "model-b"),
        ];

        let matched = resolve_test_targets(&models, "new-api").unwrap();

        assert_eq!(matched.len(), 2);
        assert_eq!(matched[0].slug, "model-a");
        assert_eq!(matched[1].slug, "model-b");
    }

    #[test]
    fn test_target_slug_wins_over_provider_name() {
        let models = vec![
            test_route("new-api", "openai", "model-a"),
            test_route("model-b", "new-api", "model-b"),
        ];

        let matched = resolve_test_targets(&models, "new-api").unwrap();

        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].slug, "new-api");
    }

    #[test]
    fn join_upstream_url_appends_expected_endpoint() {
        assert_eq!(
            join_upstream_url("https://api.example.com/v1", "/chat/completions"),
            "https://api.example.com/v1/chat/completions"
        );
        assert_eq!(
            join_upstream_url("https://api.example.com", "/messages"),
            "https://api.example.com/v1/messages"
        );
    }

    #[test]
    fn parse_export_accepts_redacted_flag() {
        let args = vec!["--without-keys".to_string(), "/tmp/models.json".to_string()];

        let command = parse_export_command(&args).unwrap();

        match command {
            CommandSpec::ExportConfig { path, without_keys } => {
                assert_eq!(path, PathBuf::from("/tmp/models.json"));
                assert!(without_keys);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_import_requires_one_path() {
        let args = vec!["/tmp/models.json".to_string()];

        let command = parse_import_command(&args).unwrap();

        match command {
            CommandSpec::ImportConfig(path) => assert_eq!(path, PathBuf::from("/tmp/models.json")),
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_accepts_app_patch_commands() {
        let patch = Args::parse(vec!["patch-app".to_string()]).unwrap();
        assert!(matches!(patch.command, CommandSpec::PatchApp));

        let restore = Args::parse(vec!["restore-app".to_string()]).unwrap();
        assert!(matches!(restore.command, CommandSpec::RestoreApp));

        let unpatch_alias = Args::parse(vec!["unpatch-app".to_string()]).unwrap();
        assert!(matches!(unpatch_alias.command, CommandSpec::RestoreApp));
    }

    #[test]
    fn backup_path_keeps_original_filename() {
        let backup = backup_path_for(Path::new("/tmp/models.json")).unwrap();

        assert!(backup
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap()
            .starts_with("models.json.bak."));
    }
}
