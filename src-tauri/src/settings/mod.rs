//! Application settings, stored as JSON values in the `settings` table.

use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, Transaction, params};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::history::{self, Action, EntityType, Source};
use crate::paths::AppPaths;

pub const DEFAULT_CONFIRMATION_WORD: &str = "CONFIRM";

fn default_trash_days() -> u32 {
    30
}

fn default_true() -> bool {
    true
}

/// Every user-editable setting. Missing keys fall back to [`Settings::default`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Folder for encrypted backups; `None` = the default under the app data dir.
    pub backup_dir: Option<String>,
    /// Word typed to allow writing ~/.ssh/config (and restoring it).
    pub confirmation_word: String,
    /// Back up automatically before risky changes (needs a remembered password).
    pub auto_backup: bool,
    /// Ordinary edits trigger at most one automatic backup per this many minutes (0 = never).
    pub auto_backup_interval_min: u32,
    /// Automatic backups to keep (manual ones are never pruned).
    pub retention_count: u32,
    /// Also delete automatic backups older than this many days (0 = no age limit).
    pub retention_days: u32,
    /// Hosts in trash longer than this many days are deleted permanently (0 = never).
    #[serde(default = "default_trash_days")]
    pub trash_retention_days: u32,
    /// Look for a new release on startup.
    #[serde(default = "default_true")]
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            backup_dir: None,
            confirmation_word: DEFAULT_CONFIRMATION_WORD.into(),
            auto_backup: true,
            auto_backup_interval_min: 30,
            retention_count: 30,
            retention_days: 0,
            trash_retention_days: default_trash_days(),
            check_updates: true,
        }
    }
}

fn get<T: DeserializeOwned>(tx: &Transaction, key: &str) -> AppResult<Option<T>> {
    let raw: Option<String> = tx
        .query_row(
            "SELECT value_json FROM settings WHERE key = ?1",
            [key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.and_then(|s| serde_json::from_str(&s).ok()))
}

fn put<T: Serialize>(tx: &Transaction, key: &str, value: &T) -> AppResult<()> {
    tx.execute(
        "INSERT INTO settings (key, value_json) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
        params![key, serde_json::to_string(value)?],
    )?;
    Ok(())
}

pub fn load(tx: &Transaction) -> AppResult<Settings> {
    let d = Settings::default();
    Ok(Settings {
        backup_dir: get::<String>(tx, "backup_dir")?.filter(|s| !s.trim().is_empty()),
        confirmation_word: get::<String>(tx, "confirmation_word")?
            .map(|w| w.trim().to_string())
            .filter(|w| !w.is_empty())
            .unwrap_or(d.confirmation_word),
        auto_backup: get(tx, "auto_backup")?.unwrap_or(d.auto_backup),
        auto_backup_interval_min: get(tx, "auto_backup_interval_min")?
            .unwrap_or(d.auto_backup_interval_min),
        retention_count: get(tx, "retention_count")?.unwrap_or(d.retention_count),
        retention_days: get(tx, "retention_days")?.unwrap_or(d.retention_days),
        trash_retention_days: get(tx, "trash_retention_days")?.unwrap_or(d.trash_retention_days),
        check_updates: get(tx, "check_updates")?.unwrap_or(d.check_updates),
    })
}

fn validate(s: &mut Settings) -> AppResult<()> {
    s.confirmation_word = s.confirmation_word.trim().to_string();
    let w = &s.confirmation_word;
    if w.chars().count() < 3
        || w.chars().count() > 32
        || w.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(AppError::Validation(
            "the confirmation word must be 3–32 characters without spaces".into(),
        ));
    }
    if s.auto_backup_interval_min > 24 * 60 {
        return Err(AppError::Validation(
            "the backup interval is at most 1440 minutes".into(),
        ));
    }
    if !(1..=1000).contains(&s.retention_count) {
        return Err(AppError::Validation(
            "keep between 1 and 1000 automatic backups".into(),
        ));
    }
    if s.trash_retention_days > 3650 {
        return Err(AppError::Validation(
            "the trash limit is at most 3650 days".into(),
        ));
    }
    if s.retention_days > 3650 {
        return Err(AppError::Validation(
            "the age limit is at most 3650 days".into(),
        ));
    }
    s.backup_dir = s
        .backup_dir
        .take()
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());
    if let Some(dir) = &s.backup_dir {
        check_writable_dir(Path::new(dir))?;
    }
    Ok(())
}

/// The folder must be absolute and writable (it may be a OneDrive/Dropbox folder).
pub fn check_writable_dir(dir: &Path) -> AppResult<()> {
    if !dir.is_absolute() {
        return Err(AppError::Validation(format!(
            "{} is not an absolute path",
            dir.display()
        )));
    }
    std::fs::create_dir_all(dir)
        .and_then(|_| {
            let probe = dir.join(format!(
                ".ridgessh-write-test-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::write(&probe, b"ok")?;
            std::fs::remove_file(&probe)
        })
        .map_err(|e| AppError::Validation(format!("cannot write to {}: {e}", dir.display())))
}

pub fn save(tx: &Transaction, mut s: Settings) -> AppResult<Settings> {
    validate(&mut s)?;
    let before = load(tx)?;
    put(tx, "backup_dir", &s.backup_dir.clone().unwrap_or_default())?;
    put(tx, "confirmation_word", &s.confirmation_word)?;
    put(tx, "auto_backup", &s.auto_backup)?;
    put(tx, "auto_backup_interval_min", &s.auto_backup_interval_min)?;
    put(tx, "retention_count", &s.retention_count)?;
    put(tx, "retention_days", &s.retention_days)?;
    put(tx, "trash_retention_days", &s.trash_retention_days)?;
    put(tx, "check_updates", &s.check_updates)?;
    let after = load(tx)?;
    if after != before {
        history::record(
            tx,
            Source::User,
            EntityType::Settings,
            "app",
            Action::Update,
            Some(&before),
            Some(&after),
        )?;
    }
    Ok(after)
}

/// The word the user types to allow a write to ~/.ssh/config.
pub fn confirmation_word(tx: &Transaction) -> AppResult<String> {
    Ok(load(tx)?.confirmation_word)
}

pub fn backup_dir(tx: &Transaction, paths: &AppPaths) -> AppResult<PathBuf> {
    Ok(load(tx)?
        .backup_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.default_backup_dir.clone()))
}

/// Small per-host conveniences (e.g. the last remote folder used for uploads). Not part of
/// [`Settings`] and not recorded in history.
pub fn pref(tx: &Transaction, key: &str) -> AppResult<Option<String>> {
    get(tx, &format!("pref:{key}"))
}

pub fn set_pref(tx: &Transaction, key: &str, value: &str) -> AppResult<()> {
    put(tx, &format!("pref:{key}"), &value)
}

#[cfg(test)]
pub fn set<T: Serialize>(tx: &Transaction, key: &str, value: &T) -> AppResult<()> {
    put(tx, key, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn defaults_validation_and_history() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.tx(load).unwrap(), Settings::default());

        let bad = |f: fn(&mut Settings)| {
            let mut s = Settings::default();
            f(&mut s);
            db.tx(|tx| save(tx, s)).is_err()
        };
        assert!(bad(|s| s.confirmation_word = "no".into()));
        assert!(bad(|s| s.confirmation_word = "two words".into()));
        assert!(bad(|s| s.retention_count = 0));
        assert!(bad(|s| s.backup_dir = Some("relative/dir".into())));

        let dir = tempfile::tempdir().unwrap();
        let wanted = Settings {
            backup_dir: Some(dir.path().join("bk").display().to_string()),
            confirmation_word: " WRITE ".into(),
            auto_backup: false,
            auto_backup_interval_min: 0,
            retention_count: 5,
            retention_days: 7,
            trash_retention_days: 0,
            check_updates: false,
        };
        let saved = db.tx(|tx| save(tx, wanted.clone())).unwrap();
        assert_eq!(saved.confirmation_word, "WRITE");
        assert!(dir.path().join("bk").is_dir());
        assert_eq!(db.tx(load).unwrap(), saved);
        let log = db
            .tx(|tx| history::for_entity(tx, EntityType::Settings, "app", 10))
            .unwrap();
        assert_eq!(log.len(), 1);
    }
}
