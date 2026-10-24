//! Audit log: every data change with full before/after snapshots.
//!
//! Writes happen inside the same transaction as the change they describe, and every
//! transaction is one batch (`batch_id`), so a whole user action (bulk delete, config scan,
//! restore) can be reviewed and undone together. See [`revert`] for undo.

pub mod revert;

use std::cell::RefCell;

use rusqlite::{Row, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::db::{new_id, now};
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    User,
    Sync,
    #[allow(dead_code)] // M8
    Import,
    /// Restores from a backup and undo from history.
    Restore,
    /// Housekeeping RidgeSSH does on its own (emptying old trash).
    System,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Host,
    Group,
    Tag,
    Settings,
    Backup,
    Script,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Create,
    Update,
    Delete,
    Restore,
    Purge,
}

fn as_str<T: Serialize>(v: T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

thread_local! {
    static BATCH: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Marks the current thread's transaction as one batch until dropped (see `Db::tx`).
pub struct BatchGuard;

pub fn begin_batch() -> BatchGuard {
    BATCH.with(|b| *b.borrow_mut() = Some(new_id()));
    BatchGuard
}

impl Drop for BatchGuard {
    fn drop(&mut self) {
        BATCH.with(|b| *b.borrow_mut() = None);
    }
}

/// Records one change. `before`/`after` are full snapshots of the entity (or `None`).
pub fn record<B: Serialize, A: Serialize>(
    tx: &Transaction,
    source: Source,
    entity_type: EntityType,
    entity_id: &str,
    action: Action,
    before: Option<&B>,
    after: Option<&A>,
) -> AppResult<()> {
    let before = before.map(serde_json::to_string).transpose()?;
    let after = after.map(serde_json::to_string).transpose()?;
    let batch = BATCH.with(|b| b.borrow().clone());
    tx.execute(
        "INSERT INTO audit_log (ts, batch_id, source, entity_type, entity_id, action, before_json, after_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            now(),
            batch,
            as_str(source),
            as_str(entity_type),
            entity_id,
            as_str(action),
            before,
            after
        ],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: i64,
    pub ts: String,
    pub batch_id: Option<String>,
    pub source: String,
    pub entity_type: String,
    pub entity_id: String,
    pub action: String,
    /// Human name of the entity (host alias, group name, ...).
    pub label: String,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
}

const SELECT: &str =
    "SELECT id, ts, batch_id, source, entity_type, entity_id, action, before_json, after_json
     FROM audit_log";

fn from_row(r: &Row) -> rusqlite::Result<AuditEntry> {
    let parse = |s: Option<String>| s.and_then(|s| serde_json::from_str(&s).ok());
    let before: Option<serde_json::Value> = parse(r.get(7)?);
    let after: Option<serde_json::Value> = parse(r.get(8)?);
    let entity_type: String = r.get(4)?;
    let entity_id: String = r.get(5)?;
    let name = |v: &Option<serde_json::Value>| {
        v.as_ref()
            .and_then(|v| v.get("alias").or_else(|| v.get("name")))
            .and_then(|n| n.as_str())
            .map(str::to_owned)
    };
    let label = match entity_type.as_str() {
        "settings" => "Settings".to_string(),
        _ => name(&after)
            .or_else(|| name(&before))
            .unwrap_or_else(|| entity_id.clone()),
    };
    Ok(AuditEntry {
        id: r.get(0)?,
        ts: r.get(1)?,
        batch_id: r.get(2)?,
        source: r.get(3)?,
        entity_type,
        entity_id,
        action: r.get(6)?,
        label,
        before,
        after,
    })
}

/// Newest-first history for one entity.
pub fn for_entity(
    tx: &Transaction,
    entity_type: EntityType,
    entity_id: &str,
    limit: u32,
) -> AppResult<Vec<AuditEntry>> {
    let mut stmt = tx.prepare(&format!(
        "{SELECT} WHERE entity_type = ?1 AND entity_id = ?2 ORDER BY id DESC LIMIT ?3"
    ))?;
    let rows = stmt.query_map(params![as_str(entity_type), entity_id, limit], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get(tx: &Transaction, id: i64) -> AppResult<AuditEntry> {
    use rusqlite::OptionalExtension;
    tx.query_row(&format!("{SELECT} WHERE id = ?1"), [id], from_row)
        .optional()?
        .ok_or_else(|| crate::error::AppError::NotFound(format!("history entry {id}")))
}

/// Entries of one batch, oldest first.
pub fn batch(tx: &Transaction, batch_id: &str) -> AppResult<Vec<AuditEntry>> {
    let mut stmt = tx.prepare(&format!("{SELECT} WHERE batch_id = ?1 ORDER BY id"))?;
    let rows = stmt.query_map([batch_id], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Filter {
    pub entity_type: Option<EntityType>,
    pub source: Option<Source>,
    /// Matches anywhere in the snapshots (alias, hostname, notes, ...).
    pub search: Option<String>,
    /// Page cursor: only entries older than this id.
    pub before_id: Option<i64>,
    pub limit: Option<u32>,
}

/// Newest-first history across everything, filtered and paged.
pub fn list(tx: &Transaction, f: &Filter) -> AppResult<Vec<AuditEntry>> {
    let search = f
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            let escaped = s
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            format!("%{escaped}%")
        });
    let mut stmt = tx.prepare(&format!(
        "{SELECT}
         WHERE (?1 IS NULL OR entity_type = ?1)
           AND (?2 IS NULL OR source = ?2)
           AND (?3 IS NULL OR id < ?3)
           AND (?4 IS NULL OR before_json LIKE ?4 ESCAPE '\\' OR after_json LIKE ?4 ESCAPE '\\'
                OR entity_id = ?5)
         ORDER BY id DESC LIMIT ?6"
    ))?;
    let rows = stmt.query_map(
        params![
            f.entity_type.map(as_str),
            f.source.map(as_str),
            f.before_id,
            search,
            f.search.as_deref().map(str::trim),
            f.limit.unwrap_or(100).clamp(1, 500)
        ],
        from_row,
    )?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests;
