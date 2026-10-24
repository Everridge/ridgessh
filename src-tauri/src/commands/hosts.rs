use tauri::State;

use crate::AppState;
use crate::db::hosts::{self, Host, HostInput};
use crate::error::AppResult;
use crate::history::{self, AuditEntry, EntityType, Source};

#[tauri::command]
pub fn hosts_list(state: State<'_, AppState>) -> AppResult<Vec<Host>> {
    state.db.tx(hosts::list)
}

#[tauri::command]
pub fn host_get(state: State<'_, AppState>, id: String) -> AppResult<Host> {
    state.db.tx(|tx| hosts::get(tx, &id))
}

#[tauri::command]
pub fn host_create(state: State<'_, AppState>, input: HostInput) -> AppResult<Host> {
    state.db.tx(|tx| hosts::create(tx, input, Source::User))
}

#[tauri::command]
pub fn host_update(state: State<'_, AppState>, id: String, input: HostInput) -> AppResult<Host> {
    state
        .db
        .tx(|tx| hosts::update(tx, &id, input, Source::User))
}

#[tauri::command]
pub fn host_set_favorite(state: State<'_, AppState>, id: String, favorite: bool) -> AppResult<()> {
    state
        .db
        .tx(|tx| hosts::patch_meta(tx, &[id], Some(favorite), None, Source::User))
}

#[tauri::command]
pub fn hosts_move(
    state: State<'_, AppState>,
    ids: Vec<String>,
    group_id: Option<String>,
) -> AppResult<()> {
    state
        .db
        .tx(|tx| hosts::patch_meta(tx, &ids, None, Some(group_id), Source::User))
}

#[tauri::command]
pub fn hosts_delete(state: State<'_, AppState>, ids: Vec<String>) -> AppResult<()> {
    state.db.tx(|tx| hosts::soft_delete(tx, &ids, Source::User))
}

#[tauri::command]
pub fn host_restore(state: State<'_, AppState>, id: String) -> AppResult<Host> {
    state.db.tx(|tx| hosts::restore(tx, &id, Source::User))
}

#[tauri::command]
pub fn host_purge(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.db.tx(|tx| hosts::purge(tx, &id, Source::User))
}

#[tauri::command]
pub fn host_history(
    state: State<'_, AppState>,
    id: String,
    limit: Option<u32>,
) -> AppResult<Vec<AuditEntry>> {
    state
        .db
        .tx(|tx| history::for_entity(tx, EntityType::Host, &id, limit.unwrap_or(100)))
}
