use tauri::State;

use crate::AppState;
use crate::backup::Reason;
use crate::db::hosts::Host;
use crate::error::{AppError, AppResult};
use crate::ssh_config;
use crate::sync::{self, Keep, RenameSuggestion, SyncReport};

/// Scans ~/.ssh/config (and its includes) and reconciles the database. Never writes the config.
#[tauri::command]
pub async fn sync_run(state: State<'_, AppState>) -> AppResult<SyncReport> {
    let path = &state.paths.ssh_config;
    let parsed = ssh_config::load(path)?;
    // Back up first if the scan is going to change anything.
    if state
        .db
        .dry_run(|tx| sync::run(tx, &parsed, path))?
        .changed()
    {
        state.auto_backup(Reason::Sync)?;
    }
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

/// Per-user `Match originalhost <alias> user <name>` blocks in the live config, read fresh
/// so they follow edits made outside RidgeSSH. Read-only; never writes the config.
#[tauri::command]
pub async fn config_variants(
    state: State<'_, AppState>,
) -> AppResult<Vec<ssh_config::UserVariant>> {
    Ok(ssh_config::load(&state.paths.ssh_config)?.variants)
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
pub async fn sync_acknowledge(state: State<'_, AppState>, ids: Vec<String>) -> AppResult<()> {
    state.auto_backup(Reason::Periodic)?;
    state.db.tx(|tx| sync::acknowledge(tx, &ids))
}

#[tauri::command]
pub async fn sync_keep_db_only(state: State<'_, AppState>, ids: Vec<String>) -> AppResult<()> {
    state.auto_backup(Reason::Periodic)?;
    state.db.tx(|tx| sync::keep_db_only(tx, &ids))
}

#[tauri::command]
pub async fn sync_resolve_conflict(
    state: State<'_, AppState>,
    id: String,
    keep: Keep,
) -> AppResult<Host> {
    state.auto_backup(Reason::Periodic)?;
    state.db.tx(|tx| sync::resolve_conflict(tx, &id, keep))
}

#[tauri::command]
pub async fn sync_accept_rename(
    state: State<'_, AppState>,
    from_id: String,
    to_id: String,
) -> AppResult<Host> {
    state.auto_backup(Reason::Periodic)?;
    state.db.tx(|tx| sync::accept_rename(tx, &from_id, &to_id))
}
