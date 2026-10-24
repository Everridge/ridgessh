use rusqlite::{Transaction, params};
use serde::Serialize;

use crate::db::new_id;
use crate::error::{AppError, AppResult};
use crate::history::{self, Action, EntityType, Source};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagWithCount {
    #[serde(flatten)]
    pub tag: Tag,
    pub host_count: i64,
}

pub fn list(tx: &Transaction) -> AppResult<Vec<TagWithCount>> {
    let mut stmt = tx.prepare(
        "SELECT t.id, t.name, t.color,
                (SELECT COUNT(*) FROM host_tags ht JOIN hosts h ON h.id = ht.host_id
                  WHERE ht.tag_id = t.id AND h.deleted_at IS NULL)
         FROM tags t ORDER BY t.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(TagWithCount {
            tag: Tag {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
            },
            host_count: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn normalize_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 || name.chars().any(char::is_control) {
        return Err(AppError::Validation(format!("invalid tag name: {name:?}")));
    }
    Ok(name.to_string())
}

/// Returns tags for `names`, creating missing ones (case-insensitive match).
pub fn ensure(tx: &Transaction, names: &[String], source: Source) -> AppResult<Vec<Tag>> {
    let mut out: Vec<Tag> = Vec::new();
    for raw in names {
        let name = normalize_name(raw)?;
        if out.iter().any(|t| t.name.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let existing = tx
            .query_row(
                "SELECT id, name, color FROM tags WHERE name = ?1 COLLATE NOCASE",
                [&name],
                |r| {
                    Ok(Tag {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        color: r.get(2)?,
                    })
                },
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(e),
            })?;
        let tag = match existing {
            Some(t) => t,
            None => {
                let tag = Tag {
                    id: new_id(),
                    name,
                    color: None,
                };
                tx.execute(
                    "INSERT INTO tags (id, name, color) VALUES (?1, ?2, NULL)",
                    params![tag.id, tag.name],
                )?;
                history::record(
                    tx,
                    source,
                    EntityType::Tag,
                    &tag.id,
                    Action::Create,
                    None::<&Tag>,
                    Some(&tag),
                )?;
                tag
            }
        };
        out.push(tag);
    }
    Ok(out)
}

pub fn delete(tx: &Transaction, id: &str, source: Source) -> AppResult<()> {
    let tag = tx
        .query_row(
            "SELECT id, name, color FROM tags WHERE id = ?1",
            [id],
            |r| {
                Ok(Tag {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    color: r.get(2)?,
                })
            },
        )
        .map_err(|_| AppError::NotFound(format!("tag {id}")))?;
    tx.execute("DELETE FROM tags WHERE id = ?1", [id])?;
    history::record(
        tx,
        source,
        EntityType::Tag,
        id,
        Action::Delete,
        Some(&tag),
        None::<&Tag>,
    )
}
