use serde::Serialize;
use tauri::State;

use crate::{AppState, error::AppResult};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub db_file: String,
    pub default_backup_dir: String,
    pub ssh_config: String,
    pub ssh_config_exists: bool,
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppResult<AppInfo> {
    let p = &state.paths;
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        data_dir: p.data_dir.display().to_string(),
        db_file: p.db_file.display().to_string(),
        default_backup_dir: p.default_backup_dir.display().to_string(),
        ssh_config: p.ssh_config.display().to_string(),
        ssh_config_exists: p.ssh_config.is_file(),
    })
}
