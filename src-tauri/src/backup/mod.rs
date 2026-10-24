//! Encrypted backups (M5).
//!
//! A backup is one `.rsbak` file: a zip with `manifest.json`, a consistent snapshot of the
//! database (`db.sqlite`, via `VACUUM INTO`) and the ssh config files, encrypted with an
//! `age` passphrase (scrypt). Private keys are never included.
//!
//! The file name says when, why and whether it was made by hand, e.g.
//! `ridgessh-20261024T135400.123Z-auto-config-write.rsbak`, so the folder alone is the list
//! of backups: it survives restoring an older database and works for folders synced from
//! another machine.

pub mod auto;
pub mod password;
pub mod restore;

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use age::secrecy::SecretString;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config_write::atomic;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::paths::AppPaths;
use crate::settings::{self, Settings};
use crate::ssh_config;
pub use password::SecretStore;

pub const EXTENSION: &str = "rsbak";
const PREFIX: &str = "ridgessh-";
const FORMAT: u32 = 1;
/// scrypt cost (2^18 ≈ 256 MiB, about a second). Decryption accepts up to 2^20.
const WORK_FACTOR: u8 = 18;
const MAX_WORK_FACTOR: u8 = 20;
const TS_FORMAT: &str = "%Y%m%dT%H%M%S%.3fZ";

/// Everything backup code needs; built from `AppState` (or by tests).
pub struct Ctx<'a> {
    pub db: &'a Db,
    pub paths: &'a AppPaths,
    pub secrets: &'a dyn SecretStore,
    /// Lower scrypt cost for tests.
    pub work_factor: u8,
}

impl<'a> Ctx<'a> {
    pub fn new(db: &'a Db, paths: &'a AppPaths, secrets: &'a dyn SecretStore) -> Self {
        Self {
            db,
            paths,
            secrets,
            work_factor: WORK_FACTOR,
        }
    }

    pub fn settings(&self) -> AppResult<Settings> {
        self.db.tx(settings::load)
    }

    pub fn backup_dir(&self) -> AppResult<PathBuf> {
        self.db.tx(|tx| settings::backup_dir(tx, self.paths))
    }

    /// Scratch space under the app data dir, removed when dropped.
    fn scratch(&self) -> AppResult<Scratch> {
        let dir = self
            .paths
            .data_dir
            .join("tmp")
            .join(uuid::Uuid::new_v4().simple().to_string());
        std::fs::create_dir_all(&dir)?;
        Ok(Scratch(dir))
    }
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Why a backup was made. Stored in the file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    Manual,
    ConfigWrite,
    BeforeRestore,
    BulkDelete,
    Sync,
    Periodic,
    #[allow(dead_code)] // M8
    Import,
    /// Before undoing changes from history.
    Revert,
}

impl Reason {
    pub fn slug(self) -> &'static str {
        match self {
            Reason::Manual => "manual",
            Reason::ConfigWrite => "config-write",
            Reason::BeforeRestore => "before-restore",
            Reason::BulkDelete => "bulk-delete",
            Reason::Sync => "sync",
            Reason::Periodic => "periodic",
            Reason::Import => "import",
            Reason::Revert => "revert",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    pub name: String,
    pub sha256: String,
    pub size: u64,
    /// Where the file came from (ssh config files).
    pub original_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: u32,
    pub app_version: String,
    pub created_at: String,
    pub reason: String,
    pub manual: bool,
    pub host_count: u64,
    pub group_count: u64,
    pub config_path: String,
    pub files: Vec<ManifestFile>,
}

/// A backup file in the folder.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub file_name: String,
    pub path: String,
    pub created_at: String,
    /// Reason slug, or `unknown` for a renamed file.
    pub reason: String,
    /// Manual backups (and files RidgeSSH did not name) are never pruned.
    pub manual: bool,
    pub size: u64,
}

fn file_name(ts: DateTime<Utc>, manual: bool, reason: Reason) -> String {
    let kind = if manual { "manual" } else { "auto" };
    let reason = if manual {
        String::new()
    } else {
        format!("-{}", reason.slug())
    };
    format!(
        "{PREFIX}{}-{kind}{reason}.{EXTENSION}",
        ts.format(TS_FORMAT)
    )
}

/// `(created_at, manual, reason)` from a RidgeSSH file name.
fn parse_name(name: &str) -> Option<(DateTime<Utc>, bool, String)> {
    let stem = name
        .strip_prefix(PREFIX)?
        .strip_suffix(&format!(".{EXTENSION}"))?;
    let (ts, rest) = stem.split_once('-')?;
    let ts = NaiveDateTime::parse_from_str(ts, TS_FORMAT).ok()?.and_utc();
    match rest.split_once('-') {
        None if rest == "manual" => Some((ts, true, "manual".into())),
        Some(("auto", reason)) => Some((ts, false, reason.into())),
        _ => None,
    }
}

/// Backups in `dir`, newest first. A missing folder has none.
pub fn list(dir: &Path) -> AppResult<Vec<BackupInfo>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if path.extension().is_none_or(|e| e != EXTENSION) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let (ts, manual, reason) = parse_name(&name).unwrap_or_else(|| {
            let mtime = meta
                .modified()
                .map(DateTime::<Utc>::from)
                .unwrap_or_default();
            (mtime, true, "unknown".into())
        });
        out.push(BackupInfo {
            file_name: name,
            path: path.display().to_string(),
            created_at: ts.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            reason,
            manual,
            size: meta.len(),
        });
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn encrypt(plain: &[u8], password: &SecretString, work_factor: u8) -> AppResult<Vec<u8>> {
    let mut recipient = age::scrypt::Recipient::new(password.clone());
    recipient.set_work_factor(work_factor);
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
            .map_err(|e| AppError::Other(format!("encryption failed: {e}")))?;
    let mut out = Vec::with_capacity(plain.len() + 1024);
    let mut w = encryptor.wrap_output(&mut out)?;
    w.write_all(plain)?;
    w.finish()?;
    Ok(out)
}

fn decrypt(data: &[u8], password: &SecretString) -> AppResult<Vec<u8>> {
    let not_backup = || AppError::Validation("this is not a RidgeSSH backup file".into());
    let decryptor = age::Decryptor::new_buffered(data).map_err(|_| not_backup())?;
    if !decryptor.is_scrypt() {
        return Err(not_backup());
    }
    let mut identity = age::scrypt::Identity::new(password.clone());
    identity.set_max_work_factor(MAX_WORK_FACTOR);
    let mut reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|e| match e {
            age::DecryptError::DecryptionFailed | age::DecryptError::NoMatchingKeys => {
                AppError::Validation("wrong password for this backup".into())
            }
            e => AppError::Validation(format!("cannot decrypt the backup: {e}")),
        })?;
    let mut out = Vec::new();
    reader
        .read_to_end(&mut out)
        .map_err(|e| AppError::Validation(format!("the backup file is damaged: {e}")))?;
    Ok(out)
}

/// Decrypted, checksum-verified contents of a backup.
pub struct Bundle {
    pub manifest: Manifest,
    pub files: BTreeMap<String, Vec<u8>>,
}

impl Bundle {
    pub fn read(path: &Path, password: &SecretString) -> AppResult<Self> {
        let plain = decrypt(&std::fs::read(path)?, password)?;
        let damaged = |what: String| AppError::Validation(format!("the backup is damaged: {what}"));
        let mut zip =
            zip::ZipArchive::new(Cursor::new(plain)).map_err(|e| damaged(e.to_string()))?;
        let mut entry = |name: &str| -> AppResult<Vec<u8>> {
            let mut f = zip
                .by_name(name)
                .map_err(|_| damaged(format!("{name} is missing")))?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)
                .map_err(|e| damaged(e.to_string()))?;
            Ok(buf)
        };
        let manifest: Manifest = serde_json::from_slice(&entry("manifest.json")?)
            .map_err(|e| damaged(format!("manifest: {e}")))?;
        if manifest.format > FORMAT {
            return Err(AppError::Validation(
                "this backup was made by a newer RidgeSSH version".into(),
            ));
        }
        let mut files = BTreeMap::new();
        for f in &manifest.files {
            let data = entry(&f.name)?;
            if sha256_hex(&data) != f.sha256 {
                return Err(damaged(format!("checksum mismatch for {}", f.name)));
            }
            files.insert(f.name.clone(), data);
        }
        if !files.contains_key("db.sqlite") {
            return Err(damaged("no database".into()));
        }
        Ok(Self { manifest, files })
    }

    /// The main ssh config file, if the backup has one.
    pub fn main_config(&self) -> Option<&[u8]> {
        self.files.get("ssh/config").map(Vec::as_slice)
    }
}

/// Makes a backup now and writes it into the backup folder.
pub fn create(ctx: &Ctx, password: &SecretString, reason: Reason) -> AppResult<BackupInfo> {
    let manual = reason == Reason::Manual;
    let dir = ctx.backup_dir()?;
    settings::check_writable_dir(&dir)?;

    let scratch = ctx.scratch()?;
    let db_file = scratch.0.join("db.sqlite");
    ctx.db.snapshot_to(&db_file)?;
    let (host_count, group_count) = ctx.db.tx(|tx| {
        let count = |sql: &str| {
            tx.query_row(sql, [], |r| r.get::<_, i64>(0))
                .map(|n| n as u64)
        };
        Ok((
            count("SELECT COUNT(*) FROM hosts WHERE deleted_at IS NULL")?,
            count("SELECT COUNT(*) FROM groups")?,
        ))
    })?;

    let mut entries: Vec<(String, Vec<u8>, Option<String>)> =
        vec![("db.sqlite".into(), std::fs::read(&db_file)?, None)];
    // The main config and everything it includes, as they are on disk.
    let config = &ctx.paths.ssh_config;
    if config.is_file() {
        entries.push((
            "ssh/config".into(),
            std::fs::read(config)?,
            Some(config.display().to_string()),
        ));
        let parsed = ssh_config::load(config)?;
        for (i, f) in parsed
            .files
            .iter()
            .filter(|f| f.path != *config)
            .enumerate()
        {
            let name = f
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Ok(bytes) = std::fs::read(&f.path) {
                entries.push((
                    format!("ssh/include-{i}-{name}"),
                    bytes,
                    Some(f.path.display().to_string()),
                ));
            }
        }
    }

    let now = Utc::now();
    let manifest = Manifest {
        format: FORMAT,
        app_version: env!("CARGO_PKG_VERSION").into(),
        created_at: now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        reason: reason.slug().into(),
        manual,
        host_count,
        group_count,
        config_path: config.display().to_string(),
        files: entries
            .iter()
            .map(|(name, data, original)| ManifestFile {
                name: name.clone(),
                sha256: sha256_hex(data),
                size: data.len() as u64,
                original_path: original.clone(),
            })
            .collect(),
    };

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let zip_err =
        |e: zip::result::ZipError| AppError::Other(format!("cannot build the backup: {e}"));
    zip.start_file("manifest.json", options).map_err(zip_err)?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    for (name, data, _) in &entries {
        zip.start_file(name.as_str(), options).map_err(zip_err)?;
        zip.write_all(data)?;
    }
    let plain = zip.finish().map_err(zip_err)?.into_inner();
    let encrypted = encrypt(&plain, password, ctx.work_factor)?;

    let name = file_name(now, manual, reason);
    let path = dir.join(&name);
    atomic::replace_file(&path, &encrypted)?;
    tracing::info!(path = %path.display(), reason = reason.slug(), "backup created");
    Ok(BackupInfo {
        file_name: name,
        path: path.display().to_string(),
        created_at: manifest.created_at,
        reason: reason.slug().into(),
        manual,
        size: encrypted.len() as u64,
    })
}

/// Deletes automatic backups beyond the retention settings. The newest backup is always
/// kept, and manual ones are never touched. Returns how many files were removed.
pub fn prune(dir: &Path, s: &Settings) -> AppResult<usize> {
    let cutoff = (s.retention_days > 0)
        .then(|| Utc::now() - chrono::Duration::days(i64::from(s.retention_days)));
    let mut removed = 0;
    for (i, b) in list(dir)?.iter().filter(|b| !b.manual).enumerate() {
        let too_many = i >= s.retention_count as usize;
        let too_old = i > 0
            && cutoff
                .is_some_and(|c| DateTime::parse_from_rfc3339(&b.created_at).is_ok_and(|t| t < c));
        if (too_many || too_old) && std::fs::remove_file(&b.path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Deletes one backup file in the backup folder (only `.rsbak` files there).
pub fn delete(dir: &Path, file_name: &str) -> AppResult<()> {
    let path = resolve_in(dir, file_name)?;
    std::fs::remove_file(path)?;
    Ok(())
}

/// A file name from the UI, confined to the backup folder.
pub fn resolve_in(dir: &Path, file_name: &str) -> AppResult<PathBuf> {
    let plain = Path::new(file_name)
        .file_name()
        .map(|n| n == file_name)
        .unwrap_or(false);
    if !plain || !file_name.ends_with(&format!(".{EXTENSION}")) {
        return Err(AppError::Validation(format!(
            "invalid backup name {file_name:?}"
        )));
    }
    let path = dir.join(file_name);
    if !path.is_file() {
        return Err(AppError::NotFound(format!("backup {file_name}")));
    }
    Ok(path)
}

#[cfg(test)]
mod tests;
