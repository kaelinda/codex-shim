use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::error::{AppError, AppResult};
use crate::paths::detect_project_root;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliOutput {
    pub command: String,
    pub args: Vec<String>,
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub ok: bool,
}

#[derive(Debug, Clone)]
pub struct ShimInvocation {
    pub program: PathBuf,
    pub leading_args: Vec<String>,
    pub project_root: Option<PathBuf>,
}

impl ShimInvocation {
    /// Resolve which command to call. Order of preference:
    /// 1. user-provided override (absolute path or command on PATH);
    /// 2. `codex-shim` on PATH;
    /// 3. fall back to `python3 -m codex_shim.cli` (or `py -3.11`) running from
    ///    the detected project root.
    pub fn resolve(override_cmd: Option<&str>, project_root: Option<&Path>) -> AppResult<Self> {
        let project_root = detect_project_root(project_root);

        if let Some(cmd) = override_cmd {
            let trimmed = cmd.trim();
            if !trimmed.is_empty() {
                let path = PathBuf::from(trimmed);
                if path.exists() {
                    return Ok(Self {
                        program: path,
                        leading_args: vec![],
                        project_root,
                    });
                }
                if let Ok(found) = which::which(trimmed) {
                    return Ok(Self {
                        program: found,
                        leading_args: vec![],
                        project_root,
                    });
                }
            }
        }

        if let Ok(found) = which::which("codex-shim") {
            return Ok(Self {
                program: found,
                leading_args: vec![],
                project_root,
            });
        }

        let python_candidates: &[&str] = if cfg!(windows) {
            &["py", "python", "python3"]
        } else {
            &["python3", "python"]
        };
        for cand in python_candidates {
            if let Ok(found) = which::which(cand) {
                let mut leading = vec![];
                if cand == &"py" {
                    leading.push("-3.11".to_string());
                }
                leading.extend(["-m".to_string(), "codex_shim.cli".to_string()]);
                return Ok(Self {
                    program: found,
                    leading_args: leading,
                    project_root,
                });
            }
        }

        Err(AppError::msg(
            "找不到 codex-shim CLI 也找不到 python，请在 Settings 里手动指定 CLI 路径或安装 codex-shim",
        ))
    }

    pub async fn run(
        &self,
        settings_path: &Path,
        port: u16,
        subcommand: &[&str],
    ) -> AppResult<CliOutput> {
        let mut args: Vec<String> = self.leading_args.clone();
        args.push("--settings".into());
        args.push(settings_path.display().to_string());
        args.push("--port".into());
        args.push(port.to_string());
        for s in subcommand {
            args.push((*s).to_string());
        }

        let mut command = Command::new(&self.program);
        command
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(root) = &self.project_root {
            command.current_dir(root);
            // Make sure the embedded python module is importable when we fell
            // back to `python -m codex_shim.cli`.
            let pythonpath = std::env::var("PYTHONPATH").unwrap_or_default();
            let extended = if pythonpath.is_empty() {
                root.display().to_string()
            } else {
                #[cfg(windows)]
                let sep = ";";
                #[cfg(not(windows))]
                let sep = ":";
                format!("{}{}{}", root.display(), sep, pythonpath)
            };
            command.env("PYTHONPATH", extended);
        }

        let output = command.output().await?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let status = output.status.code();

        Ok(CliOutput {
            command: self.program.display().to_string(),
            args,
            status,
            stdout,
            stderr,
            ok: output.status.success(),
        })
    }
}
