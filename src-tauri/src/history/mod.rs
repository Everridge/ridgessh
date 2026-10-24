//! Audit log (every data change) and activity log (sessions, transfers, scripts).
//!
//! Writes happen inside the same transaction as the change they describe.
//! The history UI and revert land in M6.

use rusqlite::{Transaction, params};
use serde::Serialize;

use crate::db::now;
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    User,
    #[allow(dead_code)] // M2
    Sync,
    #[allow(dead_code)] // M8
    Import,
    Restore,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Host,
    Group,
    Tag,
    Settings,
    Backup,
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
    tx.execute(
        "INSERT INTO audit_log (ts, batch_id, source, entity_type, entity_id, action, before_json, after_json)
         VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            now(),
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: i64,
    pub ts: String,
    pub batch_id: Option<String>,
    pub source: String,
    pub entity_type: String,
    pub entity_id: String,
    pub action: String,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
}

/// Newest-first history for one entity.
pub fn for_entity(
    tx: &Transaction,
    entity_type: EntityType,
    entity_id: &str,
    limit: u32,
) -> AppResult<Vec<AuditEntry>> {
    let mut stmt = tx.prepare(
        "SELECT id, ts, batch_id, source, entity_type, entity_id, action, before_json, after_json
         FROM audit_log WHERE entity_type = ?1 AND entity_id = ?2
         ORDER BY id DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![as_str(entity_type), entity_id, limit], |r| {
        let parse = |s: Option<String>| s.and_then(|s| serde_json::from_str(&s).ok());
        Ok(AuditEntry {
            id: r.get(0)?,
            ts: r.get(1)?,
            batch_id: r.get(2)?,
            source: r.get(3)?,
            entity_type: r.get(4)?,
            entity_id: r.get(5)?,
            action: r.get(6)?,
            before: parse(r.get(7)?),
            after: parse(r.get(8)?),
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
