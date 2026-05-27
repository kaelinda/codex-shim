use std::path::PathBuf;

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

pub fn app_runtime_dir() -> PathBuf {
    home_dir().join(".codex-shim").join("cli")
}

pub fn catalog_path() -> PathBuf {
    app_runtime_dir().join("custom_model_catalog.json")
}

pub fn generated_config_path() -> PathBuf {
    app_runtime_dir().join("config.toml")
}

pub fn codex_config_backup_path() -> PathBuf {
    app_runtime_dir().join("config.toml.before-codex-shim")
}

pub fn pid_path() -> PathBuf {
    app_runtime_dir().join("shim.pid")
}

pub fn log_path() -> PathBuf {
    app_runtime_dir().join("shim.log")
}
