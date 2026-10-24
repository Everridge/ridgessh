use tauri::State;

use crate::AppState;
use crate::backup::Reason;
use crate::error::AppResult;
use crate::history::revert::{self, Target};
use crate::history::{self, AuditEntry, Filter};

/// Newest-first history across hosts, groups, settings and backups.
#[tauri::command]
pub fn history_list(state: State<'_, AppState>, filter: Filter) -> AppResult<Vec<AuditEntry>> {
    state.db.tx(|tx| history::list(tx, &filter))
}

/// Undo one change (`before`) or go back to the version it produced (`after`).
#[tauri::command]
pub async fn history_revert(state: State<'_, AppState>, id: i64, target: Target) -> AppResult<()> {
    state.auto_backup(Reason::Revert)?;
    state.db.tx(|tx| revert::revert_entry(tx, id, target))
}

/// Undo everything one action changed (e.g. a bulk delete or a config scan).
#[tauri::command]
pub async fn history_revert_batch(
    state: State<'_, AppState>,
    batch_id: String,
) -> AppResult<usize> {
    state.auto_backup(Reason::Revert)?;
    state.db.tx(|tx| revert::revert_batch(tx, &batch_id))
}
