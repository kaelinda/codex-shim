use std::cmp::Ordering;
use std::env;
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::AppResult;

const DEFAULT_REPO: &str = "kaelinda/codex-shim";
const DEFAULT_REF: &str = "feature/cli";
const USER_AGENT_VALUE: &str = "codex-shim-updater";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub latest_tag: String,
    pub update_available: bool,
    pub repo: String,
    pub release_url: String,
    pub release_notes: String,
    pub assets: Vec<ReleaseAsset>,
    pub install_ref: String,
    pub install_command: String,
    pub checked_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCommandOutput {
    pub command: String,
    pub args: Vec<String>,
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub ok: bool,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    name: Option<String>,
    html_url: String,
    body: Option<String>,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

pub async fn check_latest_release() -> AppResult<UpdateInfo> {
    let repo = default_repo_slug();
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let response = http_client().get(url).send().await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(fallback_update_info(None));
    }
    let release = response.error_for_status()?;
    let release = release.json::<GitHubRelease>().await?;
    let latest_version = normalize_version(&release.tag_name);
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let update_available = compare_versions(&latest_version, &current_version) == Ordering::Greater;
    let install_ref = release.tag_name.clone();
    Ok(UpdateInfo {
        current_version,
        latest_version,
        latest_tag: release.tag_name.clone(),
        update_available,
        repo: repo.clone(),
        release_url: release.html_url,
        release_notes: release.body.or(release.name).unwrap_or_default(),
        assets: release
            .assets
            .into_iter()
            .map(|asset| ReleaseAsset {
                name: asset.name,
                download_url: asset.browser_download_url,
            })
            .collect(),
        install_command: install_command(&repo, &install_ref),
        install_ref,
        checked_at: now_secs(),
    })
}

pub async fn install_cli_update(ref_name: Option<&str>) -> AppResult<UpdateCommandOutput> {
    let repo = default_repo_slug();
    let ref_name = ref_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_REF);
    let start_url = start_script_url(&repo, ref_name);
    let script = http_client()
        .get(&start_url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let clone_url = env::var("CODEX_SHIM_REPO").unwrap_or_else(|_| format!("https://github.com/{repo}.git"));
    let mut child = Command::new("bash")
        .arg("-s")
        .env("CODEX_SHIM_REPO", clone_url)
        .env("CODEX_SHIM_REF", ref_name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(script.as_bytes()).await?;
    }
    let output = child.wait_with_output().await?;
    Ok(UpdateCommandOutput {
        command: "bash".to_string(),
        args: vec!["-s".to_string()],
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        ok: output.status.success(),
    })
}

#[allow(dead_code)]
pub fn fallback_update_info(ref_name: Option<&str>) -> UpdateInfo {
    let repo = default_repo_slug();
    let install_ref = ref_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_REF)
        .to_string();
    UpdateInfo {
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        latest_version: String::new(),
        latest_tag: String::new(),
        update_available: false,
        repo: repo.clone(),
        release_url: format!("https://github.com/{repo}/releases"),
        release_notes: "未找到 GitHub latest release，已回退到仓库 Releases 页面。".to_string(),
        assets: Vec::new(),
        install_command: install_command(&repo, &install_ref),
        install_ref,
        checked_at: now_secs(),
    }
}

#[allow(dead_code)]
pub fn default_update_ref() -> &'static str {
    DEFAULT_REF
}

fn http_client() -> reqwest::Client {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    headers.insert(ACCEPT, HeaderValue::from_static("application/vnd.github+json"));
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .expect("static updater headers are valid")
}

fn install_command(repo: &str, ref_name: &str) -> String {
    format!(
        "CODEX_SHIM_REF={ref_name} bash -c \"$(curl -fsSL {})\"",
        start_script_url(repo, ref_name)
    )
}

fn start_script_url(repo: &str, ref_name: &str) -> String {
    format!("https://raw.githubusercontent.com/{repo}/{ref_name}/start.sh")
}

fn default_repo_slug() -> String {
    env::var("CODEX_SHIM_UPDATE_REPO")
        .or_else(|_| env::var("CODEX_SHIM_REPO"))
        .ok()
        .and_then(|value| normalize_repo_slug(&value))
        .unwrap_or_else(|| DEFAULT_REPO.to_string())
}

fn normalize_repo_slug(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let mut repo = if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        rest.to_string()
    } else if let Some(rest) = trimmed.strip_prefix("https://github.com/") {
        rest.to_string()
    } else if let Some(rest) = trimmed.strip_prefix("http://github.com/") {
        rest.to_string()
    } else {
        trimmed.to_string()
    };
    if let Some(stripped) = repo.strip_suffix(".git") {
        repo = stripped.to_string();
    }
    let parts = repo.split('/').collect::<Vec<_>>();
    if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
        Some(format!("{}/{}", parts[0], parts[1]))
    } else {
        None
    }
}

fn normalize_version(tag: &str) -> String {
    tag.trim().trim_start_matches('v').trim_start_matches('V').to_string()
}

fn compare_versions(a: &str, b: &str) -> Ordering {
    let left = version_numbers(a);
    let right = version_numbers(b);
    let width = left.len().max(right.len());
    for idx in 0..width {
        let l = left.get(idx).copied().unwrap_or(0);
        let r = right.get(idx).copied().unwrap_or(0);
        match l.cmp(&r) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

fn version_numbers(value: &str) -> Vec<u64> {
    normalize_version(value)
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u64>().ok())
        .collect()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_slug_accepts_common_github_urls() {
        assert_eq!(
            normalize_repo_slug("git@github.com:kaelinda/codex-shim.git").as_deref(),
            Some("kaelinda/codex-shim")
        );
        assert_eq!(
            normalize_repo_slug("https://github.com/kaelinda/codex-shim.git").as_deref(),
            Some("kaelinda/codex-shim")
        );
        assert_eq!(
            normalize_repo_slug("kaelinda/codex-shim").as_deref(),
            Some("kaelinda/codex-shim")
        );
    }

    #[test]
    fn version_compare_handles_v_prefix_and_patch_width() {
        assert_eq!(compare_versions("v0.4.0", "0.3.9"), Ordering::Greater);
        assert_eq!(compare_versions("0.3", "0.3.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.2.9", "0.3.0"), Ordering::Less);
    }

    #[test]
    fn fallback_info_uses_branch_installer() {
        let info = fallback_update_info(None);
        assert_eq!(info.install_ref, DEFAULT_REF);
        assert_eq!(info.release_url, "https://github.com/kaelinda/codex-shim/releases");
        assert!(info.release_notes.contains("latest release"));
        assert!(info.install_command.contains("start.sh"));
    }
}
