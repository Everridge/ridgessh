//! Key/value application settings stored as JSON in the `settings` table.
//!
//! M4 only reads the values the config writer needs; the settings screen lands in M5.

use std::path::PathBuf;

use rusqlite::{OptionalExtension, Transaction};
use serde::de::DeserializeOwned;

use crate::error::AppResult;
use crate::paths::AppPaths;

pub const DEFAULT_CONFIRMATION_WORD: &str = "CONFIRM";

pub fn get<T: DeserializeOwned>(tx: &Transaction, key: &str) -> AppResult<Option<T>> {
    let raw: Option<String> = tx
        .query_row(
            "SELECT value_json FROM settings WHERE key = ?1",
            [key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.and_then(|s| serde_json::from_str(&s).ok()))
}

#[cfg(test)]
pub fn set<T: serde::Serialize>(tx: &Transaction, key: &str, value: &T) -> AppResult<()> {
    tx.execute(
        "INSERT INTO settings (key, value_json) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
        rusqlite::params![key, serde_json::to_string(value)?],
    )?;
    Ok(())
}

/// The word the user types to allow a write to ~/.ssh/config.
pub fn confirmation_word(tx: &Transaction) -> AppResult<String> {
    Ok(get::<String>(tx, "confirmation_word")?
        .map(|w| w.trim().to_string())
        .filter(|w| !w.is_empty())
        .unwrap_or_else(|| DEFAULT_CONFIRMATION_WORD.to_string()))
}

pub fn backup_dir(tx: &Transaction, paths: &AppPaths) -> AppResult<PathBuf> {
    Ok(get::<String>(tx, "backup_dir")?
        .filter(|d| !d.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.default_backup_dir.clone()))
}
