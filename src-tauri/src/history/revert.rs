//! Undo from history: put an entity back to the state before a change ("undo this change")
//! or after it ("restore this version"), or undo a whole batch.
//!
//! Reverts go through the normal repositories, so they are validated, recorded (source
//! `restore`) and themselves undoable. Connection fields are restored in the database only:
//! if they differ from ~/.ssh/config the host shows a pending config change, and the file is
//! written only through the guarded writer.

use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use super::{Action, AuditEntry, EntityType, Source};
use crate::db::groups::{self, GroupInput};
use crate::db::hosts::{self, Directive, HostField, HostInput};
use crate::db::now;
use crate::error::{AppError, AppResult};
use crate::settings::{self, Settings};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// The state before the change (undo it).
    Before,
    /// The state right after the change (go back to this version).
    After,
}

/// The parts of a host snapshot that a revert restores.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct HostSnapshot {
    alias: String,
    hostname: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<String>,
    proxy_jump: Option<String>,
    extra_options: Vec<Directive>,
    group_id: Option<String>,
    description: String,
    notes: String,
    color: Option<String>,
    favorite: bool,
    tags: Vec<TagName>,
    fields: Vec<HostField>,
    deleted_at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct TagName {
    name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct GroupSnapshot {
    name: String,
    parent_id: Option<String>,
    color: Option<String>,
    icon: Option<String>,
}

fn entity(e: &AuditEntry) -> AppResult<EntityType> {
    serde_json::from_value(serde_json::Value::String(e.entity_type.clone()))
        .map_err(|_| AppError::Validation(format!("unknown entity type {}", e.entity_type)))
}

/// Can this entry be undone / restored at all? (Shown as disabled buttons otherwise.)
pub fn supported(e: &AuditEntry) -> bool {
    matches!(e.entity_type.as_str(), "host" | "group" | "settings")
}

fn group_exists(tx: &Transaction, id: &str) -> AppResult<bool> {
    Ok(tx
        .query_row("SELECT 1 FROM groups WHERE id = ?1", [id], |_| Ok(()))
        .optional()?
        .is_some())
}

fn host_exists(tx: &Transaction, id: &str) -> AppResult<bool> {
    Ok(tx
        .query_row("SELECT 1 FROM hosts WHERE id = ?1", [id], |_| Ok(()))
        .optional()?
        .is_some())
}

fn apply_host(tx: &Transaction, id: &str, snapshot: Option<&serde_json::Value>) -> AppResult<()> {
    let Some(value) = snapshot else {
        // The host did not exist at that point: move it to trash (never purge).
        if host_exists(tx, id)? {
            hosts::soft_delete(tx, &[id.to_string()], Source::Restore)?;
        }
        return Ok(());
    };
    let s: HostSnapshot = serde_json::from_value(value.clone())
        .map_err(|e| AppError::Other(format!("unreadable host snapshot: {e}")))?;

    let reinserted = !host_exists(tx, id)?;
    if reinserted {
        // Permanently deleted: bring the row back with its old id, as a RidgeSSH-only host.
        // The next config scan links it again if the config still has its block.
        let ts = now();
        tx.execute(
            "INSERT INTO hosts (id, alias, sync_state, created_at, updated_at)
             VALUES (?1, ?2, 'db_only', ?3, ?3)",
            params![id, s.alias, ts],
        )
        .map_err(|e| {
            if crate::db::is_unique_violation(&e) {
                AppError::Conflict(format!("a host named \"{}\" already exists", s.alias))
            } else {
                e.into()
            }
        })?;
        let created = hosts::get(tx, id)?;
        super::record(
            tx,
            Source::Restore,
            EntityType::Host,
            id,
            Action::Create,
            None::<&hosts::Host>,
            Some(&created),
        )?;
    }

    let current = hosts::get(tx, id)?;
    let trash_it = s.deleted_at.is_some();
    // Going (back) to trash: leave the fields alone, unless the row was just re-created and
    // still needs them.
    if trash_it && !reinserted {
        if current.deleted_at.is_none() {
            hosts::soft_delete(tx, &[id.to_string()], Source::Restore)?;
        }
        return Ok(());
    }
    if current.deleted_at.is_some() {
        hosts::restore(tx, id, Source::Restore)?;
    }
    let group_id = match s.group_id {
        Some(g) if group_exists(tx, &g)? => Some(g),
        _ => None,
    };
    let input = HostInput {
        alias: s.alias,
        hostname: s.hostname,
        user: s.user,
        port: s.port,
        identity_file: s.identity_file,
        proxy_jump: s.proxy_jump,
        extra_options: s.extra_options,
        group_id,
        description: s.description,
        notes: s.notes,
        color: s.color,
        favorite: s.favorite,
        tags: s.tags.into_iter().map(|t| t.name).collect(),
        fields: s.fields,
    };
    hosts::update(tx, id, input, Source::Restore)?;
    if trash_it {
        hosts::soft_delete(tx, &[id.to_string()], Source::Restore)?;
    }
    Ok(())
}

fn apply_group(tx: &Transaction, id: &str, snapshot: Option<&serde_json::Value>) -> AppResult<()> {
    let Some(value) = snapshot else {
        if group_exists(tx, id)? {
            groups::delete(tx, id, Source::Restore)?;
        }
        return Ok(());
    };
    let s: GroupSnapshot = serde_json::from_value(value.clone())
        .map_err(|e| AppError::Other(format!("unreadable group snapshot: {e}")))?;
    let parent_id = match s.parent_id {
        Some(p) if p != id && group_exists(tx, &p)? => Some(p),
        _ => None,
    };
    let input = GroupInput {
        name: s.name,
        parent_id,
        color: s.color,
        icon: s.icon,
    };
    if group_exists(tx, id)? {
        groups::update(tx, id, &input, Source::Restore)?;
    } else {
        groups::create_with_id(tx, id, &input, Source::Restore)?;
    }
    Ok(())
}

/// The entity as it is now, as a snapshot (None if it no longer exists).
fn current(tx: &Transaction, kind: EntityType, id: &str) -> AppResult<Option<serde_json::Value>> {
    Ok(match kind {
        EntityType::Host if host_exists(tx, id)? => {
            Some(serde_json::to_value(hosts::get(tx, id)?)?)
        }
        EntityType::Group if group_exists(tx, id)? => {
            Some(serde_json::to_value(groups::get(tx, id)?)?)
        }
        EntityType::Settings => Some(serde_json::to_value(settings::load(tx)?)?),
        _ => None,
    })
}

/// "Undo this change" only reverts the fields this change touched, on top of the current
/// state, so later edits survive. Whole-entity cases (create, purge) use the snapshot as is.
fn undo_snapshot(
    tx: &Transaction,
    kind: EntityType,
    e: &AuditEntry,
) -> AppResult<Option<serde_json::Value>> {
    let (Some(before), Some(after)) = (&e.before, &e.after) else {
        return Ok(e.before.clone());
    };
    let Some(mut now) = current(tx, kind, &e.entity_id)? else {
        return Ok(Some(before.clone()));
    };
    if let (Some(b), Some(n)) = (before.as_object(), now.as_object_mut()) {
        for (k, v) in b {
            if after.get(k) != Some(v) {
                n.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(Some(now))
}

fn apply(tx: &Transaction, e: &AuditEntry, target: Target) -> AppResult<()> {
    let kind = entity(e)?;
    let snapshot = match target {
        Target::Before => undo_snapshot(tx, kind, e)?,
        Target::After => e.after.clone(),
    };
    let snapshot = snapshot.as_ref();
    match kind {
        EntityType::Host => apply_host(tx, &e.entity_id, snapshot),
        EntityType::Group => apply_group(tx, &e.entity_id, snapshot),
        EntityType::Settings => {
            let value =
                snapshot.ok_or_else(|| AppError::Validation("nothing to restore".into()))?;
            let s: Settings = serde_json::from_value(value.clone())
                .map_err(|e| AppError::Other(format!("unreadable settings snapshot: {e}")))?;
            settings::save(tx, s).map(|_| ())
        }
        EntityType::Tag | EntityType::Backup | EntityType::Script => Err(AppError::Validation(
            "this kind of change cannot be undone".into(),
        )),
    }
}

pub fn revert_entry(tx: &Transaction, id: i64, target: Target) -> AppResult<()> {
    let e = super::get(tx, id)?;
    apply(tx, &e, target)
}

/// Undoes every supported change of a batch, newest first. Returns how many were undone.
pub fn revert_batch(tx: &Transaction, batch_id: &str) -> AppResult<usize> {
    let entries = super::batch(tx, batch_id)?;
    if entries.is_empty() {
        return Err(AppError::NotFound(format!("history batch {batch_id}")));
    }
    let mut n = 0;
    for e in entries.iter().rev().filter(|e| supported(e)) {
        apply(tx, e, Target::Before)?;
        n += 1;
    }
    Ok(n)
}
