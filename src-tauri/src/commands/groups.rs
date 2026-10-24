use tauri::State;

use crate::AppState;
use crate::db::groups::{self, Group, GroupInput};
use crate::error::AppResult;
use crate::history::Source;

#[tauri::command]
pub fn groups_list(state: State<'_, AppState>) -> AppResult<Vec<Group>> {
    state.db.tx(groups::list)
}

#[tauri::command]
pub fn group_create(state: State<'_, AppState>, input: GroupInput) -> AppResult<Group> {
    state.db.tx(|tx| groups::create(tx, &input, Source::User))
}

#[tauri::command]
pub fn group_update(state: State<'_, AppState>, id: String, input: GroupInput) -> AppResult<Group> {
    state
        .db
        .tx(|tx| groups::update(tx, &id, &input, Source::User))
}

#[tauri::command]
pub fn group_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.db.tx(|tx| groups::delete(tx, &id, Source::User))
}
