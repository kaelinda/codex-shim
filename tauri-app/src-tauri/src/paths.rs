use std::path::{Path, PathBuf};

pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 8765;

pub fn home_dir() -> PathBuf {
    directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn default_settings_path() -> PathBuf {
    home_dir().join(".codex-shim").join("models.json")
}

pub fn codex_auth_path() -> PathBuf {
    home_dir().join(".codex").join("auth.json")
}

pub fn codex_config_path() -> PathBuf {
    home_dir().join(".codex").join("config.toml")
}

/// Project root that contains the `codex_shim/` python package. We try a few
/// likely locations so the GUI works whether the user opens it from inside the
/// `tauri-app/` checkout or after installing the CLI globally.
pub fn detect_project_root(override_root: Option<&Path>) -> Option<PathBuf> {
    if let Some(root) = override_root {
        if root.join("codex_shim").is_dir() {
            return Some(root.to_path_buf());
        }
    }
    // 1. ../ relative to the executable's working directory (dev usage).
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            if ancestor.join("codex_shim").is_dir() {
                return Some(ancestor.to_path_buf());
            }
        }
    }
    // 2. ~/codex-shim (canonical install path from README).
    let home_install = home_dir().join("codex-shim");
    if home_install.join("codex_shim").is_dir() {
        return Some(home_install);
    }
    None
}

pub fn runtime_dir(project_root: Option<&Path>) -> PathBuf {
    match project_root {
        Some(root) => root.join(".codex-shim"),
        None => home_dir().join(".codex-shim").join("runtime"),
    }
}

pub fn log_path(project_root: Option<&Path>) -> PathBuf {
    runtime_dir(project_root).join("shim.log")
}
