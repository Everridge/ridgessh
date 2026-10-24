use std::path::PathBuf;

use age::secrecy::SecretString;
use serde::Serialize;
use tauri::State;

use crate::AppState;
use crate::backup::restore::{self, Inspection, RestoreOptions, RestoreResult};
use crate::backup::{self, BackupInfo, Reason, password};
use crate::error::{AppError, AppResult};
use crate::settings::{self, Settings};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub settings: Settings,
    /// The folder actually used (the default when none is set).
    pub backup_dir: String,
    pub default_backup_dir: String,
}

fn view(state: &AppState, settings: Settings) -> AppResult<SettingsView> {
    let backup_dir = state.backup_ctx().backup_dir()?;
    Ok(SettingsView {
        settings,
        backup_dir: backup_dir.display().to_string(),
        default_backup_dir: state.paths.default_backup_dir.display().to_string(),
    })
}

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> AppResult<SettingsView> {
    let s = state.db.tx(settings::load)?;
    view(&state, s)
}

#[tauri::command]
pub fn settings_update(state: State<'_, AppState>, settings: Settings) -> AppResult<SettingsView> {
    let s = state.db.tx(|tx| settings::save(tx, settings))?;
    view(&state, s)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatus {
    pub password_remembered: bool,
    pub auto_backup: bool,
    pub backup_dir: String,
    pub last_backup: Option<BackupInfo>,
    /// Why the last automatic backup failed.
    pub last_error: Option<String>,
}

#[tauri::command]
pub fn backup_status(state: State<'_, AppState>) -> AppResult<BackupStatus> {
    let ctx = state.backup_ctx();
    let dir = ctx.backup_dir()?;
    Ok(BackupStatus {
        password_remembered: state.secrets.get()?.is_some(),
        auto_backup: ctx.settings()?.auto_backup,
        backup_dir: dir.display().to_string(),
        last_backup: backup::list(&dir)?.into_iter().next(),
        last_error: state
            .backup_error
            .lock()
            .map_err(|_| AppError::Other("lock poisoned".into()))?
            .clone(),
    })
}

/// Remembers the backup password in the OS credential store (enables automatic backups).
#[tauri::command]
pub fn backup_password_remember(state: State<'_, AppState>, password: String) -> AppResult<()> {
    password::check_strength(&password)?;
    state.secrets.set(&password)
}

#[tauri::command]
pub fn backup_password_forget(state: State<'_, AppState>) -> AppResult<()> {
    state.secrets.delete()
}

#[tauri::command]
pub fn backups_list(state: State<'_, AppState>) -> AppResult<Vec<BackupInfo>> {
    backup::list(&state.backup_ctx().backup_dir()?)
}

/// The password for a new backup: the one given, else the remembered one.
fn password_for_new(state: &AppState, given: Option<String>) -> AppResult<SecretString> {
    match given {
        Some(p) => {
            password::check_strength(&p)?;
            Ok(SecretString::from(p))
        }
        None => state.secrets.get()?.map(SecretString::from).ok_or_else(|| {
            AppError::Validation("enter a backup password (none is remembered)".into())
        }),
    }
}

/// Manual backup, never pruned. Uses the remembered password unless one is given.
#[tauri::command]
pub async fn backup_create(
    state: State<'_, AppState>,
    password: Option<String>,
) -> AppResult<BackupInfo> {
    let pw = password_for_new(&state, password)?;
    let ctx = state.backup_ctx();
    let info = backup::create(&ctx, &pw, Reason::Manual)?;
    backup::prune(&ctx.backup_dir()?, &ctx.settings()?)?;
    Ok(info)
}

#[tauri::command]
pub fn backup_delete(state: State<'_, AppState>, file_name: String) -> AppResult<()> {
    backup::delete(&state.backup_ctx().backup_dir()?, &file_name)
}

fn backup_file(path: &str) -> AppResult<PathBuf> {
    let p = PathBuf::from(path);
    if p.extension().is_none_or(|e| e != backup::EXTENSION) || !p.is_file() {
        return Err(AppError::Validation(format!(
            "{path} is not a .{} file",
            backup::EXTENSION
        )));
    }
    Ok(p)
}

/// Decrypts a backup and shows what restoring it would change. Read-only.
#[tauri::command]
pub async fn backup_inspect(
    state: State<'_, AppState>,
    path: String,
    password: String,
) -> AppResult<Inspection> {
    restore::inspect(
        &state.backup_ctx(),
        &backup_file(&path)?,
        &SecretString::from(password),
    )
}

#[tauri::command]
pub async fn backup_restore(
    state: State<'_, AppState>,
    path: String,
    password: String,
    options: RestoreOptions,
) -> AppResult<RestoreResult> {
    let result = restore::restore(
        &state.backup_ctx(),
        &backup_file(&path)?,
        &SecretString::from(password),
        &options,
    )?;
    *state
        .last_sync
        .lock()
        .map_err(|_| AppError::Other("lock poisoned".into()))? = Some(result.report.clone());
    Ok(result)
}
