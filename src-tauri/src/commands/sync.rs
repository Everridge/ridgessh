use tauri::State;

use crate::AppState;
use crate::db::hosts::Host;
use crate::error::{AppError, AppResult};
use crate::ssh_config;
use crate::sync::{self, Keep, RenameSuggestion, SyncReport};

/// Scans ~/.ssh/config (and its includes) and reconciles the database. Never writes the config.
#[tauri::command]
pub fn sync_run(state: State<'_, AppState>) -> AppResult<SyncReport> {
    let path = &state.paths.ssh_config;
    let parsed = ssh_config::load(path)?;
    let report = state.db.tx(|tx| sync::run(tx, &parsed, path))?;
    tracing::info!(
        added = report.added.len(),
        updated = report.updated.len(),
        removed = report.removed.len(),
        conflicts = report.conflicts.len(),
        "config scan finished"
    );
    *state
        .last_sync
        .lock()
        .map_err(|_| AppError::Other("lock poisoned".into()))? = Some(report.clone());
    Ok(report)
}

/// The report of the last scan in this session (rules and warnings for the review dialog).
#[tauri::command]
pub fn sync_last(state: State<'_, AppState>) -> AppResult<Option<SyncReport>> {
    Ok(state
        .last_sync
        .lock()
        .map_err(|_| AppError::Other("lock poisoned".into()))?
        .clone())
}

#[tauri::command]
pub fn sync_rename_suggestions(state: State<'_, AppState>) -> AppResult<Vec<RenameSuggestion>> {
    state.db.tx(sync::rename_suggestions)
}

#[tauri::command]
pub fn sync_acknowledge(state: State<'_, AppState>, ids: Vec<String>) -> AppResult<()> {
    state.db.tx(|tx| sync::acknowledge(tx, &ids))
}

#[tauri::command]
pub fn sync_keep_db_only(state: State<'_, AppState>, ids: Vec<String>) -> AppResult<()> {
    state.db.tx(|tx| sync::keep_db_only(tx, &ids))
}

#[tauri::command]
pub fn sync_resolve_conflict(
    state: State<'_, AppState>,
    id: String,
    keep: Keep,
) -> AppResult<Host> {
    state.db.tx(|tx| sync::resolve_conflict(tx, &id, keep))
}

#[tauri::command]
pub fn sync_accept_rename(
    state: State<'_, AppState>,
    from_id: String,
    to_id: String,
) -> AppResult<Host> {
    state.db.tx(|tx| sync::accept_rename(tx, &from_id, &to_id))
}
