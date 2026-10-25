//! SQLite connection, migrations and repositories.

pub mod activity;
pub mod groups;
pub mod hosts;
pub mod tags;

use std::path::Path;
use std::sync::{LazyLock, Mutex};

use rusqlite::{Connection, Transaction};
use rusqlite_migration::{M, Migrations};

use crate::error::{AppError, AppResult};

const MIGRATION_SQL: &[&str] = &[
    include_str!("migrations/001_init.sql"),
    include_str!("migrations/002_config_tracking.sql"),
    include_str!("migrations/003_drop_backups_table.sql"),
    include_str!("migrations/004_folder_recipes.sql"),
    include_str!("migrations/005_user_matches.sql"),
];

static MIGRATIONS: LazyLock<Migrations<'static>> =
    LazyLock::new(|| Migrations::new(MIGRATION_SQL.iter().map(|sql| M::up(sql)).collect()));

/// Single shared connection. The app is single-user and low-traffic, so a mutex is enough.
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let version = usize::try_from(version).unwrap_or(usize::MAX);
        if version > MIGRATION_SQL.len() {
            return Err(AppError::Other(format!(
                "the database at {} was created by a newer version of RidgeSSH;                  install that version or restore a backup",
                path.display()
            )));
        }
        if version > 0 && version < MIGRATION_SQL.len() {
            // An app update is about to change the schema: keep a copy of the old one.
            let copy = path.with_extension(format!("v{version}.bak"));
            let _ = std::fs::remove_file(&copy);
            conn.execute("VACUUM INTO ?1", [copy.to_string_lossy()])?;
            tracing::info!(from = version, copy = %copy.display(), "upgrading database schema");
        }
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> AppResult<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> AppResult<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        MIGRATIONS
            .to_latest(&mut conn)
            .map_err(|e| AppError::Other(format!("database migration failed: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Runs `f` in a transaction that is always rolled back (to ask "would this change
    /// anything?").
    pub fn dry_run<T>(&self, f: impl FnOnce(&Transaction) -> AppResult<T>) -> AppResult<T> {
        let _guard = TxGuard::enter()?;
        let _batch = crate::history::begin_batch();
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        f(&tx)
        // dropping `tx` rolls back
    }

    /// Writes a consistent copy of the whole database to `path` (which must not exist).
    pub fn snapshot_to(&self, path: &Path) -> AppResult<()> {
        let _guard = TxGuard::enter()?;
        let conn = self.lock()?;
        conn.execute("VACUUM INTO ?1", [path.to_string_lossy()])?;
        Ok(())
    }

    /// Replaces the live database with the one in `path`, after migrating that copy to the
    /// current schema. Uses SQLite's online backup, so the connection stays valid.
    pub fn restore_from(&self, path: &Path) -> AppResult<()> {
        let mut src = Connection::open(path)?;
        src.pragma_update(None, "foreign_keys", "ON")?;
        MIGRATIONS
            .to_latest(&mut src)
            .map_err(|e| AppError::Other(format!("cannot migrate the backup database: {e}")))?;
        let _guard = TxGuard::enter()?;
        let mut conn = self.lock()?;
        let backup = rusqlite::backup::Backup::new(&src, &mut conn)?;
        backup.run_to_completion(256, std::time::Duration::ZERO, None)?;
        Ok(())
    }

    fn lock(&self) -> AppResult<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| AppError::Other("db lock poisoned".into()))
    }

    /// Runs `f` inside a transaction; commits on `Ok`, rolls back on `Err`.
    ///
    /// Not re-entrant: calling `tx` from inside `f` would deadlock on the mutex, so it is
    /// rejected with an error instead.
    pub fn tx<T>(&self, f: impl FnOnce(&Transaction) -> AppResult<T>) -> AppResult<T> {
        let _guard = TxGuard::enter()?;
        // Everything recorded in one transaction is one undoable batch.
        let _batch = crate::history::begin_batch();
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| AppError::Other("db lock poisoned".into()))?;
        let tx = conn.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }
}

thread_local! {
    static IN_TX: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Marks the current thread as inside `Db::tx` for the guard's lifetime.
struct TxGuard;

impl TxGuard {
    fn enter() -> AppResult<Self> {
        if IN_TX.with(|f| f.replace(true)) {
            return Err(AppError::Other(
                "nested database transaction (Db::tx called inside Db::tx)".into(),
            ));
        }
        Ok(Self)
    }
}

impl Drop for TxGuard {
    fn drop(&mut self) {
        IN_TX.with(|f| f.set(false));
    }
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// True when `e` is a UNIQUE constraint violation.
pub fn is_unique_violation(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(err, _)
            if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn migrations_are_valid() {
        super::MIGRATIONS.validate().unwrap();
    }

    #[test]
    fn upgrade_keeps_a_copy_and_newer_databases_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("db.sqlite");
        {
            let mut conn = rusqlite::Connection::open(&file).unwrap();
            rusqlite_migration::Migrations::new(vec![rusqlite_migration::M::up(
                super::MIGRATION_SQL[0],
            )])
            .to_latest(&mut conn)
            .unwrap();
        }
        drop(super::Db::open(&file).unwrap());
        assert!(dir.path().join("db.v1.bak").is_file());

        let conn = rusqlite::Connection::open(&file).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        drop(conn);
        let err = super::Db::open(&file).err().unwrap().to_string();
        assert!(err.contains("newer version"), "{err}");
    }

    #[test]
    fn nested_tx_errors_instead_of_deadlocking() {
        let db = super::Db::open_in_memory().unwrap();
        let inner = db.tx(|_| Ok(db.tx(|_| Ok(())))).unwrap();
        assert!(inner.is_err());
        // The guard is released afterwards.
        db.tx(|_| Ok(())).unwrap();
    }
}
