//! Saved folder transfers ("recipes"): a name plus a [`FolderSpec`], run again with one click.

use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::Serialize;

use super::{FolderSpec, check_spec};
use crate::db::{new_id, now};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub spec: FolderSpec,
    pub created_at: String,
    pub updated_at: String,
    pub last_run_at: Option<String>,
    /// 0 = finished, other = failed; `None` after a run = cancelled.
    pub last_exit_code: Option<i64>,
}

const COLUMNS: &str = "id, name, spec_json, created_at, updated_at, last_run_at, last_exit_code";

fn from_row(r: &Row) -> rusqlite::Result<Recipe> {
    let json: String = r.get(2)?;
    let spec = serde_json::from_str(&json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Recipe {
        id: r.get(0)?,
        name: r.get(1)?,
        spec,
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
        last_run_at: r.get(5)?,
        last_exit_code: r.get(6)?,
    })
}

pub fn list(tx: &Transaction) -> AppResult<Vec<Recipe>> {
    let mut stmt = tx.prepare(&format!(
        "SELECT {COLUMNS} FROM folder_recipes ORDER BY name COLLATE NOCASE"
    ))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get(tx: &Transaction, id: &str) -> AppResult<Recipe> {
    tx.query_row(
        &format!("SELECT {COLUMNS} FROM folder_recipes WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("recipe {id}")))
}

fn check_name(tx: &Transaction, name: &str, except: Option<&str>) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(AppError::Validation(
            "give the recipe a name (up to 80 characters)".into(),
        ));
    }
    let taken: Option<String> = tx
        .query_row(
            "SELECT id FROM folder_recipes WHERE name = ?1 COLLATE NOCASE",
            [name],
            |r| r.get(0),
        )
        .optional()?;
    if taken.is_some_and(|id| Some(id.as_str()) != except) {
        return Err(AppError::Conflict(format!(
            "a recipe called {name:?} already exists"
        )));
    }
    Ok(name.to_string())
}

/// Creates a recipe (`id` = None) or replaces one's name and settings.
pub fn save(
    tx: &Transaction,
    id: Option<&str>,
    name: &str,
    spec: &FolderSpec,
) -> AppResult<Recipe> {
    check_spec(spec)?;
    let name = check_name(tx, name, id)?;
    let json = serde_json::to_string(spec).map_err(|e| AppError::Other(e.to_string()))?;
    let ts = now();
    let id = match id {
        Some(id) => {
            let n = tx.execute(
                "UPDATE folder_recipes SET name = ?2, spec_json = ?3, updated_at = ?4 WHERE id = ?1",
                params![id, name, json, ts],
            )?;
            if n == 0 {
                return Err(AppError::NotFound(format!("recipe {id}")));
            }
            id.to_string()
        }
        None => {
            let id = new_id();
            tx.execute(
                "INSERT INTO folder_recipes (id, name, spec_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![id, name, json, ts],
            )?;
            id
        }
    };
    get(tx, &id)
}

pub fn delete(tx: &Transaction, id: &str) -> AppResult<()> {
    tx.execute("DELETE FROM folder_recipes WHERE id = ?1", [id])?;
    Ok(())
}

/// A run started (`code` None) or ended.
pub fn record_run(tx: &Transaction, id: &str, code: Option<i64>) -> AppResult<()> {
    tx.execute(
        "UPDATE folder_recipes SET last_run_at = ?2, last_exit_code = ?3 WHERE id = ?1",
        params![id, now(), code],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::folder::{Endpoint, Existing};

    fn spec(dir: &std::path::Path) -> FolderSpec {
        FolderSpec {
            source: Endpoint::Remote {
                host_id: "h".into(),
                user: Some("deploy".into()),
                path: "/srv/app".into(),
            },
            destination: Endpoint::Local {
                path: dir.display().to_string(),
            },
            unpack: true,
            keep_zip: false,
            keep_copy: None,
            existing: Existing::Merge,
            owner: None,
            excludes: vec!["node_modules".into()],
            sudo_source: true,
            sudo_dest: false,
            before: None,
            after: None,
        }
    }

    #[test]
    fn save_list_rename_run_delete() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let r = db
            .tx(|tx| save(tx, None, " Prod uploads ", &spec(dir.path())))
            .unwrap();
        assert_eq!(r.name, "Prod uploads");
        assert_eq!(r.spec.excludes, ["node_modules"]);
        assert!(r.spec.sudo_source);

        // Names are unique, ignoring case; renaming to its own name is fine.
        assert!(
            db.tx(|tx| save(tx, None, "prod UPLOADS", &spec(dir.path())))
                .is_err()
        );
        let renamed = db
            .tx(|tx| save(tx, Some(&r.id), "Prod uploads", &spec(dir.path())))
            .unwrap();
        assert_eq!(renamed.id, r.id);

        db.tx(|tx| record_run(tx, &r.id, Some(0))).unwrap();
        let listed = db.tx(list).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].last_exit_code, Some(0));
        assert!(listed[0].last_run_at.is_some());

        // Invalid settings are refused.
        let mut bad = spec(dir.path());
        bad.excludes = vec!["../etc".into()];
        assert!(db.tx(|tx| save(tx, None, "bad", &bad)).is_err());

        db.tx(|tx| delete(tx, &r.id)).unwrap();
        assert!(db.tx(list).unwrap().is_empty());
    }
}
