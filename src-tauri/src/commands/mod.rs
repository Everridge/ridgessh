//! `#[tauri::command]` handlers — thin wrappers that delegate to domain modules.

pub mod app;
pub mod backup;
pub mod config;
pub mod groups;
pub mod history;
pub mod hosts;
pub mod jobs;
pub mod sync;
pub mod tags;
pub mod terminal;
pub mod transfer;
