//! Activity log: terminal sessions, transfers and script runs (not data changes — those are
//! in the audit log).

use rusqlite::{Transaction, params};
use serde::Serialize;

use crate::db::now;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: i64,
    pub host_id: Option<String>,
    pub kind: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub exit_code: Option<i64>,
    pub details: Option<serde_json::Value>,
}

/// Records the start of an activity. Terminal sessions also bump `last_connected_at`.
pub fn start(
    tx: &Transaction,
    host_id: &str,
    kind: &str,
    details: serde_json::Value,
) -> AppResult<i64> {
    let ts = now();
    tx.execute(
        "INSERT INTO activity (host_id, kind, started_at, details_json) VALUES (?1, ?2, ?3, ?4)",
        params![host_id, kind, ts, details.to_string()],
    )?;
    let id = tx.last_insert_rowid();
    if kind.starts_with("terminal") {
        tx.execute(
            "UPDATE hosts SET last_connected_at = ?2 WHERE id = ?1",
            params![host_id, ts],
        )?;
    }
    Ok(id)
}

pub fn finish(tx: &Transaction, id: i64, exit_code: Option<i64>) -> AppResult<()> {
    tx.execute(
        "UPDATE activity SET ended_at = ?2, exit_code = ?3 WHERE id = ?1 AND ended_at IS NULL",
        params![id, now(), exit_code],
    )?;
    Ok(())
}

/// Sessions still marked open from a previous run (app crashed or was killed) are closed.
pub fn close_dangling(tx: &Transaction) -> AppResult<usize> {
    Ok(tx.execute(
        "UPDATE activity SET ended_at = started_at WHERE ended_at IS NULL",
        [],
    )?)
}

pub fn for_host(tx: &Transaction, host_id: &str, limit: u32) -> AppResult<Vec<Activity>> {
    let mut stmt = tx.prepare(
        "SELECT id, host_id, kind, started_at, ended_at, exit_code, details_json
         FROM activity WHERE host_id = ?1 ORDER BY id DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![host_id, limit], |r| {
        let details: Option<String> = r.get(6)?;
        Ok(Activity {
            id: r.get(0)?,
            host_id: r.get(1)?,
            kind: r.get(2)?,
            started_at: r.get(3)?,
            ended_at: r.get(4)?,
            exit_code: r.get(5)?,
            details: details.and_then(|d| serde_json::from_str(&d).ok()),
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::db::hosts::{self, HostInput};
    use crate::history::Source;

    #[test]
    fn records_sessions_and_last_connected() {
        let db = Db::open_in_memory().unwrap();
        db.tx(|tx| {
            let h = hosts::create(
                tx,
                HostInput {
                    alias: "a".into(),
                    ..Default::default()
                },
                Source::User,
            )?;
            let id = start(tx, &h.id, "terminal", serde_json::json!({ "args": ["a"] }))?;
            assert!(hosts::get(tx, &h.id)?.last_connected_at.is_some());
            finish(tx, id, Some(255))?;
            let log = for_host(tx, &h.id, 10)?;
            assert_eq!(log.len(), 1);
            assert_eq!(log[0].exit_code, Some(255));
            assert!(log[0].ended_at.is_some());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn closes_dangling_sessions() {
        let db = Db::open_in_memory().unwrap();
        db.tx(|tx| {
            let h = hosts::create(
                tx,
                HostInput {
                    alias: "a".into(),
                    ..Default::default()
                },
                Source::User,
            )?;
            start(tx, &h.id, "terminal", serde_json::Value::Null)?;
            assert_eq!(close_dangling(tx)?, 1);
            assert!(for_host(tx, &h.id, 1)?[0].ended_at.is_some());
            Ok(())
        })
        .unwrap();
    }
}
