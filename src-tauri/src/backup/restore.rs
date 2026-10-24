//! Restoring a backup: preview what would change, then restore the database, the ssh config,
//! or both. A backup of the current state is always made first, and the config part needs
//! the confirmation word like any other config write.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use age::secrecy::SecretString;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{BackupInfo, Bundle, Ctx, Manifest, Reason};
use crate::config_write::{self, DiffLine, atomic};
use crate::db::Db;
use crate::db::hosts::{self, Host};
use crate::error::{AppError, AppResult};
use crate::history::{self, Action, EntityType, Source};
use crate::settings;
use crate::ssh_config;
use crate::sync::{self, SyncReport};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostChanges {
    /// Live hosts in the backup.
    pub total: usize,
    /// In the backup, not in RidgeSSH now (restoring brings them back).
    pub added: Vec<String>,
    /// In RidgeSSH now, not in the backup (restoring removes them).
    pub removed: Vec<String>,
    /// In both, with different data.
    pub changed: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigChanges {
    pub diff: Vec<DiffLine>,
    pub additions: usize,
    pub deletions: usize,
    /// Hash of the current file; restore refuses if it changed after the preview.
    pub base_hash: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub path: String,
    pub manifest: Manifest,
    pub hosts: HostChanges,
    /// `None` when the backup has no ssh config.
    pub config: Option<ConfigChanges>,
    pub config_path: String,
    pub confirmation_word: String,
}

/// What identifies a host's content for the preview (timestamps and sync state ignored).
fn content(h: &Host) -> impl PartialEq + use<> {
    let tags: Vec<String> = h.tags.iter().map(|t| t.name.to_lowercase()).collect();
    (
        h.connection(),
        h.description.clone(),
        h.notes.clone(),
        h.group_id.clone(),
        h.favorite,
        tags,
        h.fields.clone(),
    )
}

fn compare(current: &[Host], backup: &[Host]) -> HostChanges {
    let live = |hs: &[Host]| -> BTreeMap<String, Host> {
        hs.iter()
            .filter(|h| h.deleted_at.is_none())
            .map(|h| (h.id.clone(), h.clone()))
            .collect()
    };
    let (now, then) = (live(current), live(backup));
    let mut out = HostChanges {
        total: then.len(),
        ..Default::default()
    };
    for (id, h) in &then {
        match now.get(id) {
            None => out.added.push(h.alias.clone()),
            Some(c) if content(c) != content(h) => out.changed.push(h.alias.clone()),
            Some(_) => {}
        }
    }
    out.removed = now
        .iter()
        .filter(|(id, _)| !then.contains_key(*id))
        .map(|(_, h)| h.alias.clone())
        .collect();
    for v in [&mut out.added, &mut out.removed, &mut out.changed] {
        v.sort_by_key(|a| a.to_lowercase());
    }
    out
}

/// Hosts in the backup's database (migrated to the current schema in a scratch copy).
fn backup_hosts(ctx: &Ctx, bundle: &Bundle) -> AppResult<Vec<Host>> {
    let scratch = ctx.scratch()?;
    let path = scratch.0.join("restore.sqlite");
    std::fs::write(&path, &bundle.files["db.sqlite"])?;
    let db = Db::open(&path)?;
    let hosts = db.tx(hosts::list)?;
    drop(db);
    Ok(hosts)
}

fn current_config(path: &Path) -> AppResult<(String, String)> {
    let bytes = match std::fs::read(path) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let hash = bytes
        .as_deref()
        .map_or_else(|| "absent".into(), |b| hex::encode(Sha256::digest(b)));
    let text = String::from_utf8_lossy(bytes.as_deref().unwrap_or_default()).into_owned();
    Ok((text, hash))
}

pub fn inspect(ctx: &Ctx, path: &Path, password: &SecretString) -> AppResult<Inspection> {
    let bundle = Bundle::read(path, password)?;
    let current = ctx.db.tx(hosts::list)?;
    let hosts = compare(&current, &backup_hosts(ctx, &bundle)?);
    let config = match bundle.main_config() {
        Some(bytes) => {
            let (now, base_hash) = current_config(&ctx.paths.ssh_config)?;
            let then = String::from_utf8_lossy(bytes);
            let (diff, additions, deletions) = config_write::diff_lines(&now, &then);
            Some(ConfigChanges {
                diff,
                additions,
                deletions,
                base_hash,
            })
        }
        None => None,
    };
    Ok(Inspection {
        path: path.display().to_string(),
        manifest: bundle.manifest,
        hosts,
        config,
        config_path: ctx.paths.ssh_config.display().to_string(),
        confirmation_word: ctx.db.tx(settings::confirmation_word)?,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOptions {
    pub restore_db: bool,
    pub restore_config: bool,
    /// From the preview; required with `restore_config`.
    pub config_base_hash: Option<String>,
    pub confirmation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub restored_db: bool,
    pub restored_config: bool,
    /// The backup of the state just before restoring.
    pub safety_backup: BackupInfo,
    /// Plain copy of the replaced config, if any.
    pub previous_config: Option<String>,
    pub report: SyncReport,
}

pub fn restore(
    ctx: &Ctx,
    path: &Path,
    password: &SecretString,
    opts: &RestoreOptions,
) -> AppResult<RestoreResult> {
    if !opts.restore_db && !opts.restore_config {
        return Err(AppError::Validation("choose what to restore".into()));
    }
    let bundle = Bundle::read(path, password)?;
    let config_path = &ctx.paths.ssh_config;
    if opts.restore_config {
        let word = ctx.db.tx(settings::confirmation_word)?;
        if opts.confirmation.as_deref() != Some(word.as_str()) {
            return Err(AppError::Validation(
                "the confirmation word does not match".into(),
            ));
        }
        if bundle.main_config().is_none() {
            return Err(AppError::Validation(
                "this backup does not contain an ssh config".into(),
            ));
        }
        let (_, hash) = current_config(config_path)?;
        if opts.config_base_hash.as_deref() != Some(hash.as_str()) {
            return Err(AppError::Conflict(format!(
                "{} changed after the preview. Review the backup again.",
                config_path.display()
            )));
        }
    }

    // Safety net first: with the remembered password, else the one just entered.
    let safety_password = match ctx.secrets.get()? {
        Some(p) => SecretString::from(p),
        None => password.clone(),
    };
    let safety_backup = super::create(ctx, &safety_password, Reason::BeforeRestore)?;

    if opts.restore_db {
        let scratch = ctx.scratch()?;
        let file = scratch.0.join("restore.sqlite");
        std::fs::write(&file, &bundle.files["db.sqlite"])?;
        ctx.db.restore_from(&file)?;
        ctx.db.tx(crate::db::activity::close_dangling)?;
    }

    let mut previous_config = None;
    if opts.restore_config {
        let bytes = bundle.main_config().unwrap_or_default();
        if let Ok(old) = std::fs::read(config_path) {
            previous_config = Some(
                config_write::save_previous(&ctx.paths.data_dir, &old)?
                    .display()
                    .to_string(),
            );
        }
        atomic::replace_file(config_path, bytes)?;
    }

    let parsed = ssh_config::load(config_path)?;
    let report = ctx.db.tx(|tx| {
        let details: HashMap<&str, serde_json::Value> = HashMap::from([
            ("file", path.display().to_string().into()),
            ("restoredDb", opts.restore_db.into()),
            ("restoredConfig", opts.restore_config.into()),
            ("backupCreatedAt", bundle.manifest.created_at.clone().into()),
            ("safetyBackup", safety_backup.file_name.clone().into()),
        ]);
        history::record(
            tx,
            Source::Restore,
            EntityType::Backup,
            &safety_backup.file_name,
            Action::Restore,
            None::<&()>,
            Some(&details),
        )?;
        sync::run(tx, &parsed, config_path)
    })?;
    tracing::info!(
        file = %path.display(),
        db = opts.restore_db,
        config = opts.restore_config,
        "backup restored"
    );
    Ok(RestoreResult {
        restored_db: opts.restore_db,
        restored_config: opts.restore_config,
        safety_backup,
        previous_config,
        report,
    })
}
