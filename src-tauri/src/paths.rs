use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};

/// Resolved filesystem locations used by the app.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub db_file: PathBuf,
    pub default_backup_dir: PathBuf,
    pub ssh_config: PathBuf,
}

impl AppPaths {
    pub fn resolve(app: &AppHandle) -> AppResult<Self> {
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| AppError::Other(format!("cannot resolve app data dir: {e}")))?;
        std::fs::create_dir_all(&data_dir)?;
        Ok(Self {
            db_file: data_dir.join("ridgessh.db"),
            default_backup_dir: data_dir.join("backups"),
            ssh_config: default_ssh_config()?,
            data_dir,
        })
    }
}

/// `~/.ssh/config` on every platform (OpenSSH for Windows uses `%USERPROFILE%\.ssh\config`).
pub fn default_ssh_config() -> AppResult<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join(".ssh").join("config"))
        .ok_or_else(|| AppError::Other("cannot resolve home directory".into()))
}
