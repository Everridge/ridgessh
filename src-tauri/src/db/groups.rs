use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::db::{new_id, now};
use crate::error::{AppError, AppResult};
use crate::history::{self, Action, EntityType, Source};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInput {
    pub name: String,
    pub parent_id: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
}

const COLUMNS: &str = "id, parent_id, name, color, icon, sort_order, created_at, updated_at";

fn from_row(r: &Row) -> rusqlite::Result<Group> {
    Ok(Group {
        id: r.get(0)?,
        parent_id: r.get(1)?,
        name: r.get(2)?,
        color: r.get(3)?,
        icon: r.get(4)?,
        sort_order: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

pub fn list(tx: &Transaction) -> AppResult<Vec<Group>> {
    let mut stmt = tx.prepare(&format!(
        "SELECT {COLUMNS} FROM groups ORDER BY sort_order, name COLLATE NOCASE"
    ))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get(tx: &Transaction, id: &str) -> AppResult<Group> {
    tx.query_row(
        &format!("SELECT {COLUMNS} FROM groups WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("group {id}")))
}

fn validate(tx: &Transaction, id: Option<&str>, input: &GroupInput) -> AppResult<String> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err(AppError::Validation(
            "group name must be 1–100 characters".into(),
        ));
    }
    // Walk up from the new parent; reaching `id` would create a cycle.
    let mut cursor = input.parent_id.clone();
    while let Some(pid) = cursor {
        if Some(pid.as_str()) == id {
            return Err(AppError::Validation(
                "a group cannot be moved into itself".into(),
            ));
        }
        cursor = get(tx, &pid)?.parent_id;
    }
    Ok(name.to_string())
}

pub fn create(tx: &Transaction, input: &GroupInput, source: Source) -> AppResult<Group> {
    create_with_id(tx, &new_id(), input, source)
}

/// Creates a group with a given id (used when undoing its deletion).
pub fn create_with_id(
    tx: &Transaction,
    id: &str,
    input: &GroupInput,
    source: Source,
) -> AppResult<Group> {
    let name = validate(tx, None, input)?;
    let ts = now();
    let sort_order: i64 = tx.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM groups WHERE parent_id IS ?1",
        [&input.parent_id],
        |r| r.get(0),
    )?;
    let group = Group {
        id: id.to_string(),
        parent_id: input.parent_id.clone(),
        name,
        color: input.color.clone(),
        icon: input.icon.clone(),
        sort_order,
        created_at: ts.clone(),
        updated_at: ts,
    };
    tx.execute(
        &format!("INSERT INTO groups ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"),
        params![
            group.id,
            group.parent_id,
            group.name,
            group.color,
            group.icon,
            group.sort_order,
            group.created_at,
            group.updated_at
        ],
    )?;
    history::record(
        tx,
        source,
        EntityType::Group,
        &group.id,
        Action::Create,
        None::<&Group>,
        Some(&group),
    )?;
    Ok(group)
}

pub fn update(tx: &Transaction, id: &str, input: &GroupInput, source: Source) -> AppResult<Group> {
    let before = get(tx, id)?;
    let name = validate(tx, Some(id), input)?;
    tx.execute(
        "UPDATE groups SET parent_id = ?2, name = ?3, color = ?4, icon = ?5, updated_at = ?6 WHERE id = ?1",
        params![id, input.parent_id, name, input.color, input.icon, now()],
    )?;
    let after = get(tx, id)?;
    history::record(
        tx,
        source,
        EntityType::Group,
        id,
        Action::Update,
        Some(&before),
        Some(&after),
    )?;
    Ok(after)
}

/// Deletes a group. Its subgroups and hosts move up to the deleted group's parent. Each move
/// is recorded first, so undoing the batch recreates the group and moves everything back.
pub fn delete(tx: &Transaction, id: &str, source: Source) -> AppResult<()> {
    let group = get(tx, id)?;
    let children: Vec<Group> = list(tx)?
        .into_iter()
        .filter(|g| g.parent_id.as_deref() == Some(id))
        .collect();
    for child in children {
        tx.execute(
            "UPDATE groups SET parent_id = ?2, updated_at = ?3 WHERE id = ?1",
            params![child.id, group.parent_id, now()],
        )?;
        let after = get(tx, &child.id)?;
        history::record(
            tx,
            source,
            EntityType::Group,
            &child.id,
            Action::Update,
            Some(&child),
            Some(&after),
        )?;
    }
    let host_ids: Vec<String> = tx
        .prepare("SELECT id FROM hosts WHERE group_id = ?1")?
        .query_map([id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    if !host_ids.is_empty() {
        crate::db::hosts::patch_meta(tx, &host_ids, None, Some(group.parent_id.clone()), source)?;
    }
    tx.execute("DELETE FROM groups WHERE id = ?1", [id])?;
    history::record(
        tx,
        source,
        EntityType::Group,
        id,
        Action::Delete,
        Some(&group),
        None::<&Group>,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn input(name: &str, parent: Option<&str>) -> GroupInput {
        GroupInput {
            name: name.into(),
            parent_id: parent.map(Into::into),
            color: None,
            icon: None,
        }
    }

    #[test]
    fn rejects_cycles() {
        let db = Db::open_in_memory().unwrap();
        db.tx(|tx| {
            let a = create(tx, &input("A", None), Source::User)?;
            let b = create(tx, &input("B", Some(&a.id)), Source::User)?;
            let err = update(tx, &a.id, &input("A", Some(&b.id)), Source::User).unwrap_err();
            assert!(matches!(err, AppError::Validation(_)));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn delete_reparents_children() {
        let db = Db::open_in_memory().unwrap();
        db.tx(|tx| {
            let a = create(tx, &input("A", None), Source::User)?;
            let b = create(tx, &input("B", Some(&a.id)), Source::User)?;
            let c = create(tx, &input("C", Some(&b.id)), Source::User)?;
            delete(tx, &b.id, Source::User)?;
            assert_eq!(get(tx, &c.id)?.parent_id.as_deref(), Some(a.id.as_str()));
            Ok(())
        })
        .unwrap();
    }
}
