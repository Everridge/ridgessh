//! Reconciliation between ~/.ssh/config and the database (M2).
//!
//! The config is the source of truth for connection fields; the DB owns everything else.
//! Scanning never writes the config file and never deletes hosts:
//! - config host unknown to the DB → inserted as `config_only_new`
//! - config changed and no pending DB edits → DB connection updated from config
//! - config changed while the DB has pending edits → `conflict`, both versions kept
//! - DB host whose block disappeared → `removed_from_config`
//!
//! Each DB host is matched by the alias it has *in the config* (`config_snapshot.alias`), so
//! renaming a host in the app does not make the next scan think it is a new one.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{Transaction, params};
use serde::{Deserialize, Serialize};

use crate::db::hosts::{self, Connection, Host};
use crate::db::{new_id, now, tags};
use crate::error::{AppError, AppResult};
use crate::history::{self, Action, EntityType, Source};
use crate::ssh_config::{ConfigHost, ParsedConfig, RuleBlock};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub scanned_at: String,
    pub config_path: String,
    pub config_hosts: usize,
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub removed: Vec<String>,
    pub reappeared: Vec<String>,
    pub conflicts: Vec<String>,
    pub rules: Vec<RuleBlock>,
    pub warnings: Vec<String>,
}

impl SyncReport {
    pub fn changed(&self) -> bool {
        !(self.added.is_empty()
            && self.updated.is_empty()
            && self.removed.is_empty()
            && self.reappeared.is_empty()
            && self.conflicts.is_empty())
    }
}

/// The alias a DB host is known by in the config.
fn config_key(h: &Host) -> &str {
    h.config_snapshot
        .as_ref()
        .map(|s| s.alias.as_str())
        .unwrap_or(&h.alias)
}

/// Writes the connection columns plus config tracking columns.
#[allow(clippy::too_many_arguments)]
fn write_config_state(
    tx: &Transaction,
    id: &str,
    conn: Option<&Connection>,
    snapshot: &Connection,
    ch: &ConfigHost,
    source: Option<String>,
    state: &str,
    dirty: bool,
    ts: &str,
) -> AppResult<()> {
    if let Some(c) = conn {
        tx.execute(
            "UPDATE hosts SET alias = ?2, hostname = ?3, user = ?4, port = ?5, identity_file = ?6,
                proxy_jump = ?7, extra_options = ?8, updated_at = ?9 WHERE id = ?1",
            params![
                id,
                c.alias,
                c.hostname,
                c.user,
                c.port,
                c.identity_file,
                c.proxy_jump,
                serde_json::to_string(&c.extra_options)?,
                ts
            ],
        )?;
    }
    tx.execute(
        "UPDATE hosts SET sync_state = ?2, config_dirty = ?3, config_fingerprint = ?4,
            config_snapshot = ?5, config_source = ?6, last_seen_in_config_at = ?7,
            removed_from_config_at = NULL
         WHERE id = ?1",
        params![
            id,
            state,
            dirty,
            ch.fingerprint,
            serde_json::to_string(snapshot)?,
            source,
            ts
        ],
    )?;
    Ok(())
}

/// Applies a parsed config to the database. Runs in the caller's transaction.
pub fn run(tx: &Transaction, parsed: &ParsedConfig, main_path: &Path) -> AppResult<SyncReport> {
    let ts = now();
    let mut report = SyncReport {
        scanned_at: ts.clone(),
        config_path: main_path.display().to_string(),
        config_hosts: parsed.hosts.len(),
        rules: parsed.rules.clone(),
        warnings: parsed.warnings.clone(),
        ..Default::default()
    };

    let all = hosts::list(tx)?;
    let live: Vec<&Host> = all.iter().filter(|h| h.deleted_at.is_none()).collect();
    let by_key: HashMap<&str, &Host> = live.iter().map(|h| (config_key(h), *h)).collect();
    let live_aliases: HashSet<&str> = live.iter().map(|h| h.alias.as_str()).collect();
    let trashed_keys: HashSet<&str> = all
        .iter()
        .filter(|h| h.deleted_at.is_some())
        .map(config_key)
        .collect();
    let mut seen: HashSet<&str> = HashSet::new();

    for ch in &parsed.hosts {
        let snapshot = ch.connection();
        let source = (ch.source != main_path).then(|| ch.source.display().to_string());

        let Some(&h) = by_key.get(ch.alias.as_str()) else {
            if trashed_keys.contains(ch.alias.as_str()) {
                continue; // deleted in RidgeSSH on purpose; still in the config
            }
            if live_aliases.contains(ch.alias.as_str()) {
                report.warnings.push(format!(
                    "Host {} in the config clashes with a RidgeSSH host renamed to {}; skipped",
                    ch.alias, ch.alias
                ));
                continue;
            }
            let id = insert_from_config(tx, &snapshot, ch, source, &ts)?;
            let after = hosts::get(tx, &id)?;
            history::record(
                tx,
                Source::Sync,
                EntityType::Host,
                &id,
                Action::Create,
                None::<&Host>,
                Some(&after),
            )?;
            report.added.push(ch.alias.clone());
            continue;
        };
        seen.insert(config_key(h));

        let db_conn = h.connection();
        let config_changed = h.config_fingerprint.as_deref() != Some(ch.fingerprint.as_str())
            || h.config_snapshot.as_ref() != Some(&snapshot);

        let (conn, state, dirty) = match h.sync_state.as_str() {
            // A DB-only host that the config now also defines: link if equal, else conflict.
            "db_only" if db_conn == snapshot => (None, "synced", false),
            "db_only" => (None, "conflict", true),
            _ if !config_changed && h.sync_state != "removed_from_config" => {
                tx.execute(
                    "UPDATE hosts SET last_seen_in_config_at = ?2, config_source = ?3 WHERE id = ?1",
                    params![h.id, ts, source],
                )?;
                continue;
            }
            // Pending DB edits that now equal the config are no longer pending.
            _ if h.config_dirty && db_conn == snapshot => (None, "synced", false),
            _ if h.config_dirty => (None, "conflict", true),
            "removed_from_config" => (Some(&snapshot), "synced", false),
            state => (Some(&snapshot), state, false),
        };

        write_config_state(tx, &h.id, conn, &snapshot, ch, source, state, dirty, &ts)?;
        let after = hosts::get(tx, &h.id)?;
        if after != *h {
            history::record(
                tx,
                Source::Sync,
                EntityType::Host,
                &h.id,
                Action::Update,
                Some(h),
                Some(&after),
            )?;
        }
        match (h.sync_state.as_str(), state) {
            (_, "conflict") => report.conflicts.push(h.alias.clone()),
            ("removed_from_config", _) => report.reappeared.push(h.alias.clone()),
            _ if conn.is_some() => report.updated.push(h.alias.clone()),
            _ => {}
        }
    }

    // Live hosts that were in the config but no longer are.
    for h in &live {
        let tracked = matches!(
            h.sync_state.as_str(),
            "synced" | "config_only_new" | "conflict"
        );
        if tracked && !seen.contains(config_key(h)) {
            tx.execute(
                "UPDATE hosts SET sync_state = 'removed_from_config', removed_from_config_at = ?2
                 WHERE id = ?1",
                params![h.id, ts],
            )?;
            let after = hosts::get(tx, &h.id)?;
            history::record(
                tx,
                Source::Sync,
                EntityType::Host,
                &h.id,
                Action::Update,
                Some(*h),
                Some(&after),
            )?;
            report.removed.push(h.alias.clone());
        }
    }

    Ok(report)
}

fn insert_from_config(
    tx: &Transaction,
    c: &Connection,
    ch: &ConfigHost,
    source: Option<String>,
    ts: &str,
) -> AppResult<String> {
    let id = new_id();
    tx.execute(
        "INSERT INTO hosts (id, alias, hostname, user, port, identity_file, proxy_jump, extra_options,
            sync_state, config_dirty, config_fingerprint, config_snapshot, config_source,
            last_seen_in_config_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'config_only_new', 0, ?9, ?10, ?11, ?12, ?12, ?12)",
        params![
            id,
            c.alias,
            c.hostname,
            c.user,
            c.port,
            c.identity_file,
            c.proxy_jump,
            serde_json::to_string(&c.extra_options)?,
            ch.fingerprint,
            serde_json::to_string(c)?,
            source,
            ts
        ],
    )?;
    Ok(id)
}

// ---------------------------------------------------------------------------------------------
// Review actions (user-initiated)
// ---------------------------------------------------------------------------------------------

/// Changes columns on one host and records the change.
fn change(
    tx: &Transaction,
    id: &str,
    sql: &str,
    extra: &[&dyn rusqlite::ToSql],
) -> AppResult<Host> {
    let before = hosts::get(tx, id)?;
    let mut args: Vec<&dyn rusqlite::ToSql> = vec![&id];
    args.extend_from_slice(extra);
    tx.execute(sql, args.as_slice())?;
    let after = hosts::get(tx, id)?;
    if after != before {
        history::record(
            tx,
            Source::User,
            EntityType::Host,
            id,
            Action::Update,
            Some(&before),
            Some(&after),
        )?;
    }
    Ok(after)
}

/// Marks "new from config" hosts as reviewed.
pub fn acknowledge(tx: &Transaction, ids: &[String]) -> AppResult<()> {
    for id in ids {
        change(
            tx,
            id,
            "UPDATE hosts SET sync_state = 'synced' WHERE id = ?1 AND sync_state = 'config_only_new'",
            &[],
        )?;
    }
    Ok(())
}

/// Keeps hosts that left the config as RidgeSSH-only entries.
pub fn keep_db_only(tx: &Transaction, ids: &[String]) -> AppResult<()> {
    for id in ids {
        change(
            tx,
            id,
            "UPDATE hosts SET sync_state = 'db_only', config_dirty = 0, config_fingerprint = NULL,
                config_snapshot = NULL, config_source = NULL, removed_from_config_at = NULL
             WHERE id = ?1 AND sync_state = 'removed_from_config'",
            &[],
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    /// Take the config's connection values.
    Config,
    /// Keep the DB values as a pending change for the config (written in M4).
    Database,
}

pub fn resolve_conflict(tx: &Transaction, id: &str, keep: Keep) -> AppResult<Host> {
    let h = hosts::get(tx, id)?;
    if h.sync_state != "conflict" {
        return Err(AppError::Conflict(format!(
            "{} has no conflict to resolve",
            h.alias
        )));
    }
    let snapshot = h
        .config_snapshot
        .clone()
        .ok_or_else(|| AppError::Other(format!("{} has no config snapshot", h.alias)))?;
    match keep {
        Keep::Config => change(
            tx,
            id,
            "UPDATE hosts SET alias = ?2, hostname = ?3, user = ?4, port = ?5, identity_file = ?6,
                proxy_jump = ?7, extra_options = ?8, sync_state = 'synced', config_dirty = 0,
                updated_at = ?9
             WHERE id = ?1",
            &[
                &snapshot.alias,
                &snapshot.hostname,
                &snapshot.user,
                &snapshot.port,
                &snapshot.identity_file,
                &snapshot.proxy_jump,
                &serde_json::to_string(&snapshot.extra_options)?,
                &now(),
            ],
        )
        .map_err(|e| match e {
            AppError::Db(ref err) if crate::db::is_unique_violation(err) => AppError::Conflict(
                format!("another host is already named \"{}\"", snapshot.alias),
            ),
            e => e,
        }),
        Keep::Database => change(
            tx,
            id,
            "UPDATE hosts SET sync_state = 'synced', config_dirty = 1 WHERE id = ?1",
            &[],
        ),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RenameSuggestion {
    /// Host that disappeared from the config (has the notes/history).
    pub from_id: String,
    /// Host that appeared in the config with the same target.
    pub to_id: String,
}

/// Pairs a removed host with a new one that points at the same `user@hostname:port`.
pub fn rename_suggestions(tx: &Transaction) -> AppResult<Vec<RenameSuggestion>> {
    let all = hosts::list(tx)?;
    let target = |h: &Host| {
        h.hostname.clone().map(|hn| {
            (
                hn.to_ascii_lowercase(),
                h.user.clone(),
                h.port.unwrap_or(22),
            )
        })
    };
    let mut used = HashSet::new();
    let mut out = Vec::new();
    for removed in all
        .iter()
        .filter(|h| h.deleted_at.is_none() && h.sync_state == "removed_from_config")
    {
        let Some(t) = target(removed) else { continue };
        let candidate = all.iter().find(|h| {
            h.deleted_at.is_none()
                && h.sync_state == "config_only_new"
                && !used.contains(&h.id)
                && target(h).as_ref() == Some(&t)
        });
        if let Some(new) = candidate {
            used.insert(new.id.clone());
            out.push(RenameSuggestion {
                from_id: removed.id.clone(),
                to_id: new.id.clone(),
            });
        }
    }
    Ok(out)
}

/// Moves notes, group, tags and other metadata from the old host to the renamed one, then
/// trashes the old host (its history stays available).
pub fn accept_rename(tx: &Transaction, from_id: &str, to_id: &str) -> AppResult<Host> {
    let from = hosts::get(tx, from_id)?;
    let to = hosts::get(tx, to_id)?;
    if from.sync_state != "removed_from_config" || to.sync_state != "config_only_new" {
        return Err(AppError::Conflict(
            "these hosts are no longer a rename candidate".into(),
        ));
    }
    let tag_names: Vec<String> = to
        .tags
        .iter()
        .chain(&from.tags)
        .map(|t| t.name.clone())
        .collect();
    let after = change(
        tx,
        to_id,
        "UPDATE hosts SET group_id = ?2, description = ?3, notes = ?4, color = ?5, favorite = ?6,
            last_connected_at = ?7, sync_state = 'synced', updated_at = ?8
         WHERE id = ?1",
        &[
            &from.group_id,
            &from.description,
            &from.notes,
            &from.color,
            &from.favorite,
            &from.last_connected_at,
            &now(),
        ],
    )?;
    tx.execute("DELETE FROM host_tags WHERE host_id = ?1", [to_id])?;
    for tag in tags::ensure(tx, &tag_names, Source::User)? {
        tx.execute(
            "INSERT INTO host_tags (host_id, tag_id) VALUES (?1, ?2)",
            params![to_id, tag.id],
        )?;
    }
    tx.execute(
        "INSERT INTO host_fields (host_id, key, value, sort_order)
         SELECT ?2, key, value, sort_order FROM host_fields WHERE host_id = ?1",
        params![from_id, to_id],
    )?;
    let merged = hosts::get(tx, to_id)?;
    if merged != after {
        history::record(
            tx,
            Source::User,
            EntityType::Host,
            to_id,
            Action::Update,
            Some(&after),
            Some(&merged),
        )?;
    }
    hosts::soft_delete(tx, &[from_id.to_string()], Source::User)?;
    Ok(merged)
}

#[cfg(test)]
mod tests;
