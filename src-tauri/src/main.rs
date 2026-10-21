// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod sink;
mod state;

use std::sync::Arc;
use std::time::Duration;

use pylentra_core::secrets::KeyringStore;
use tauri::{Manager, RunEvent};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

use crate::state::AppState;

const EXIT_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

fn main() {
    let app = tauri::Builder::default()
        // First, so a second launch exits before anything else starts.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([Target::new(TargetKind::LogDir { file_name: None })])
                .rotation_strategy(RotationStrategy::KeepSome(5))
                .max_file_size(5 * 1024 * 1024)
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let openssh = dirs::home_dir().map(|h| h.join(".ssh").join("known_hosts"));
            app.manage(AppState::load(
                &config_dir,
                openssh,
                Arc::new(KeyringStore),
            )?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::library_get,
            commands::startup_warnings,
            commands::connection_save,
            commands::connection_dependents,
            commands::connection_delete,
            commands::group_save,
            commands::group_delete,
            commands::settings_get,
            commands::settings_save,
            commands::session_open,
            commands::session_write,
            commands::session_resize,
            commands::session_answer,
            commands::session_close,
            commands::forward_start,
            commands::forward_stop,
            commands::known_host_forget,
            commands::log_dir,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the Tauri application");

    app.run(|app, event| {
        if let RunEvent::ExitRequested { .. } = event
            && let Some(state) = app.try_state::<AppState>()
        {
            tauri::async_runtime::block_on(state.sessions.close_all(EXIT_CLOSE_TIMEOUT));
        }
    });
}
