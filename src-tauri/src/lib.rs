mod backup;
mod commands;
mod db;
mod error;
mod history;
mod paths;
mod scp;
mod scripts;
#[allow(dead_code)] // used by ssh/scp spawning from M3 on
mod security;
mod settings;
mod ssh_config;
mod sync;
mod terminal;
mod transfer;

use tauri::Manager;

use crate::paths::AppPaths;

/// Shared state managed by Tauri and injected into commands.
pub struct AppState {
    pub paths: AppPaths,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ridgessh=info,ridgessh_lib=info".into()),
        )
        .init();

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        // Focus the existing window instead of starting a second instance.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }));
    }

    builder
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let paths = AppPaths::resolve(app.handle())?;
            tracing::info!(data_dir = %paths.data_dir.display(), "starting RidgeSSH");
            app.manage(AppState { paths });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![commands::app_info])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
