use tauri::State;

use crate::AppState;
use crate::config_write::{self, ApplyRequest, ApplyResult, Candidate, Change, Preview};
use crate::error::{AppError, AppResult};

/// Hosts with something that could be written to ~/.ssh/config.
#[tauri::command]
pub fn config_candidates(state: State<'_, AppState>) -> AppResult<Vec<Candidate>> {
    state
        .db
        .tx(|tx| config_write::candidates(tx, &state.paths.ssh_config))
}

/// The diff the selected changes would make. Read-only.
#[tauri::command]
pub fn config_preview(state: State<'_, AppState>, changes: Vec<Change>) -> AppResult<Preview> {
    state
        .db
        .tx(|tx| config_write::preview(tx, &state.paths.ssh_config, &changes))
}

/// Writes the previewed changes. Refuses unless the confirmation word matches and neither the
/// file nor the pending changes moved since the preview.
#[tauri::command]
pub fn config_apply(
    state: State<'_, AppState>,
    changes: Vec<Change>,
    base_hash: String,
    result_hash: String,
    confirmation: String,
) -> AppResult<ApplyResult> {
    let result = state.db.tx(|tx| {
        config_write::apply(
            tx,
            &state.paths,
            &state.paths.ssh_config,
            ApplyRequest {
                changes: &changes,
                base_hash: &base_hash,
                result_hash: &result_hash,
                confirmation: &confirmation,
            },
        )
    })?;
    *state
        .last_sync
        .lock()
        .map_err(|_| AppError::Other("lock poisoned".into()))? = Some(result.report.clone());
    Ok(result)
}
