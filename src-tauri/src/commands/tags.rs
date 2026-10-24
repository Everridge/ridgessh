use tauri::State;

use crate::AppState;
use crate::db::tags::{self, TagWithCount};
use crate::error::AppResult;
use crate::history::Source;

#[tauri::command]
pub fn tags_list(state: State<'_, AppState>) -> AppResult<Vec<TagWithCount>> {
    state.db.tx(tags::list)
}

#[tauri::command]
pub fn tag_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.db.tx(|tx| tags::delete(tx, &id, Source::User))
}
