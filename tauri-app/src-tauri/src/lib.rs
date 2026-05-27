mod commands;
mod catalog;
mod config;
mod embedded_shim;
mod error;
mod health;
mod models;
mod paths;
mod updater;

use commands::*;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_runtime_info,
            get_app_settings,
            update_app_settings,
            shim_status,
            shim_health,
            shim_start,
            shim_stop,
            shim_restart,
            shim_generate,
            shim_enable,
            shim_disable,
            shim_list_models,
            shim_use_model,
            shim_launch_app,
            shim_patch_app,
            shim_restore_app,
            read_models_file,
            write_models_file,
            export_models_file,
            import_models_file,
            tail_log,
            read_codex_auth,
            current_active_model,
            check_app_update,
            install_cli_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

pub mod state {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use crate::embedded_shim::EmbeddedShimState;
    use crate::paths::{default_settings_path, DEFAULT_PORT};

    pub struct AppSettings {
        pub settings_path: PathBuf,
        pub port: u16,
    }

    impl Default for AppSettings {
        fn default() -> Self {
            Self {
                settings_path: default_settings_path(),
                port: DEFAULT_PORT,
            }
        }
    }

    #[derive(Default)]
    pub struct AppState {
        pub settings: Mutex<AppSettings>,
        pub embedded_shim: EmbeddedShimState,
    }
}
