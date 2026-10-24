mod backup;
mod commands;
mod db;
mod error;
mod history;
mod paths;
mod scp;
mod scripts;
mod security;
mod settings;
mod ssh_config;
mod sync;
mod terminal;
mod transfer;

use std::sync::Mutex;

use tauri::Manager;

use crate::db::Db;
use crate::paths::AppPaths;

/// Shared state managed by Tauri and injected into commands.
pub struct AppState {
    pub paths: AppPaths,
    pub db: Db,
    pub last_sync: Mutex<Option<sync::SyncReport>>,
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
            let db = Db::open(&paths.db_file)?;
            app.manage(AppState {
                paths,
                db,
                last_sync: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::app_info,
            commands::hosts::hosts_list,
            commands::hosts::host_get,
            commands::hosts::host_create,
            commands::hosts::host_update,
            commands::hosts::host_set_favorite,
            commands::hosts::hosts_move,
            commands::hosts::hosts_delete,
            commands::hosts::host_restore,
            commands::hosts::host_purge,
            commands::hosts::host_history,
            commands::groups::groups_list,
            commands::groups::group_create,
            commands::groups::group_update,
            commands::groups::group_delete,
            commands::tags::tags_list,
            commands::tags::tag_delete,
            commands::sync::sync_run,
            commands::sync::sync_last,
            commands::sync::sync_rename_suggestions,
            commands::sync::sync_acknowledge,
            commands::sync::sync_keep_db_only,
            commands::sync::sync_resolve_conflict,
            commands::sync::sync_accept_rename,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
