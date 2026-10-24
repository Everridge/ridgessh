//! Guarded writes to ~/.ssh/config (M4).
//!
//! Nothing here runs without the user: the UI asks for a preview (diff + hashes), the user
//! types the confirmation word, and [`apply`] then
//! 1. checks the word,
//! 2. rebuilds the change from the current file and DB and refuses if either the file
//!    (`base_hash`) or the result (`result_hash`) differs from what was previewed,
//! 3. copies the current file to the backup folder,
//! 4. replaces the file atomically, keeping its permissions (see [`atomic`]),
//! 5. marks the hosts as in sync and records the change in history, in one transaction.
//!
//! Only the main config file is written. Hosts that come from `Include`d files stay
//! read-only.

pub mod atomic;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::{Transaction, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use similar::{ChangeTag, TextDiff};

use crate::db::hosts::{self, Host};
use crate::db::now;
use crate::error::{AppError, AppResult};
use crate::history::{self, Action, EntityType, Source};
use crate::paths::AppPaths;
use crate::settings;
use crate::ssh_config::writer::{self, Edit};
use crate::ssh_config::{self, ConfigFile};
use crate::sync::{self, SyncReport};

/// How many plain copies of replaced configs to keep in `<data dir>/config-history`.
const KEEP_BACKUPS: usize = 50;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Write the host's edited connection values into its existing block.
    Update,
    /// Add a block for a RidgeSSH-only host, or restore one that was removed from the config.
    Add,
    /// Delete the block of a host that was moved to trash in RidgeSSH.
    Remove,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub host_id: String,
    pub op: Op,
}

/// A host with something that could be written to the config.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub host_id: String,
    pub alias: String,
    pub op: Op,
    /// Pre-selected in the dialog (pending edits are; additions and removals are opt-in).
    pub selected: bool,
    /// Why this change cannot be written right now.
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    /// `hunk`, `context`, `add` or `remove`.
    pub kind: &'static str,
    pub text: String,
    /// 1-based line numbers in the old / new file.
    pub old: Option<usize>,
    pub new: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub config_path: String,
    pub file_exists: bool,
    pub base_hash: String,
    pub result_hash: String,
    pub diff: Vec<DiffLine>,
    pub additions: usize,
    pub deletions: usize,
    pub confirmation_word: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub config_path: String,
    /// Plain copy of the file as it was before (in the app data folder).
    pub previous_copy: Option<String>,
    /// The encrypted automatic backup made just before, if any (set by the command).
    pub auto_backup: Option<crate::backup::BackupInfo>,
    pub changed: usize,
    pub report: SyncReport,
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The main config file as it is on disk now.
struct Current {
    bytes: Option<Vec<u8>>,
    file: ConfigFile,
    hash: String,
}

fn read_current(path: &Path) -> AppResult<Current> {
    let bytes = match std::fs::read(path) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let text = match &bytes {
        Some(b) => ssh_config::decode(path, b.clone())?,
        None => String::new(),
    };
    Ok(Current {
        hash: bytes.as_deref().map_or_else(|| "absent".into(), sha256_hex),
        file: ConfigFile::parse(path, &text),
        bytes,
    })
}

/// The op a host offers, if any, and whether it is pre-selected.
fn offered_op(h: &Host) -> Option<(Op, bool)> {
    if h.deleted_at.is_some() {
        let in_config = h.config_snapshot.is_some() && h.sync_state != "removed_from_config";
        return in_config.then_some((Op::Remove, false));
    }
    match h.sync_state.as_str() {
        "removed_from_config" => Some((Op::Add, false)),
        "db_only" => Some((Op::Add, false)),
        "conflict" => Some((Op::Update, false)),
        _ if h.config_dirty && h.config_snapshot.is_some() => Some((Op::Update, true)),
        _ => None,
    }
}

fn config_alias(h: &Host) -> &str {
    h.config_snapshot.as_ref().map_or(&h.alias, |s| &s.alias)
}

fn blocked_reason(h: &Host, op: Op, current: &Current) -> Option<String> {
    if h.sync_state == "conflict" {
        return Some("Resolve the conflict with the config first.".into());
    }
    if op == Op::Add {
        return None;
    }
    if let Some(src) = &h.config_source {
        return Some(format!(
            "Defined in the included file {src}. RidgeSSH only writes the main config."
        ));
    }
    let block = ssh_config::find_host_block(&current.file, config_alias(h));
    let fingerprint = block.map(|b| {
        ssh_config::extract_host(&current.file, b, config_alias(h), &mut Vec::new()).fingerprint
    });
    match fingerprint {
        None => Some("No longer in the config file. Re-scan the config.".into()),
        Some(fp) if h.config_fingerprint.as_deref() != Some(fp.as_str()) => Some(
            "Its block was changed in the config file since the last scan. Re-scan the config."
                .into(),
        ),
        Some(_) => None,
    }
}

/// Everything that could be written, for the dialog's checklist.
pub fn candidates(tx: &Transaction, config_path: &Path) -> AppResult<Vec<Candidate>> {
    let current = read_current(config_path)?;
    let mut out = Vec::new();
    for h in hosts::list(tx)? {
        let Some((op, selected)) = offered_op(&h) else {
            continue;
        };
        let blocked = blocked_reason(&h, op, &current);
        // A trashed host whose block is already gone has nothing left to do.
        if op == Op::Remove && blocked.is_some() {
            continue;
        }
        out.push(Candidate {
            host_id: h.id.clone(),
            alias: h.alias.clone(),
            op,
            selected: selected && blocked.is_none(),
            blocked,
        });
    }
    Ok(out)
}

struct Plan {
    current: Current,
    new_text: String,
    new_file: ConfigFile,
    changes: Vec<(Host, Op)>,
}

fn plan(tx: &Transaction, config_path: &Path, changes: &[Change]) -> AppResult<Plan> {
    if changes.is_empty() {
        return Err(AppError::Validation("nothing selected".into()));
    }
    let current = read_current(config_path)?;
    let mut edits = Vec::new();
    let mut planned = Vec::new();
    let mut seen = HashSet::new();
    for c in changes {
        if !seen.insert(&c.host_id) {
            return Err(AppError::Validation("a host is listed twice".into()));
        }
        let h = hosts::get(tx, &c.host_id)?;
        if offered_op(&h).map(|(op, _)| op) != Some(c.op) {
            return Err(AppError::Conflict(format!(
                "{} changed in the meantime. Review the changes again.",
                h.alias
            )));
        }
        if let Some(reason) = blocked_reason(&h, c.op, &current) {
            return Err(AppError::Conflict(format!("{}: {reason}", h.alias)));
        }
        edits.push(match c.op {
            Op::Update => Edit::Update {
                alias: config_alias(&h).to_string(),
                conn: h.connection(),
            },
            Op::Add => Edit::Add {
                conn: h.connection(),
            },
            Op::Remove => Edit::Remove {
                alias: config_alias(&h).to_string(),
            },
        });
        planned.push((h, c.op));
    }

    // Hosts from included files: a new block must not shadow them.
    let included: HashSet<String> = ssh_config::load(config_path)?
        .hosts
        .into_iter()
        .filter(|h| h.source != config_path)
        .map(|h| h.alias)
        .collect();
    let new_text = writer::apply(&current.file, &edits, &included)?;
    let new_file = writer::verify(&current.file, &new_text, &edits)?;
    Ok(Plan {
        current,
        new_text,
        new_file,
        changes: planned,
    })
}

pub fn diff_lines(old: &str, new: &str) -> (Vec<DiffLine>, usize, usize) {
    let diff = TextDiff::from_lines(old, new);
    let (mut out, mut adds, mut dels) = (Vec::new(), 0, 0);
    for group in diff.grouped_ops(3) {
        let (first, last) = (&group[0], &group[group.len() - 1]);
        let (o, n) = (first.old_range(), first.new_range());
        out.push(DiffLine {
            kind: "hunk",
            text: format!(
                "@@ -{},{} +{},{} @@",
                o.start + 1,
                last.old_range().end - o.start,
                n.start + 1,
                last.new_range().end - n.start
            ),
            old: None,
            new: None,
        });
        for op in &group {
            for change in diff.iter_changes(op) {
                let kind = match change.tag() {
                    ChangeTag::Equal => "context",
                    ChangeTag::Insert => {
                        adds += 1;
                        "add"
                    }
                    ChangeTag::Delete => {
                        dels += 1;
                        "remove"
                    }
                };
                out.push(DiffLine {
                    kind,
                    text: change.value().trim_end_matches(['\n', '\r']).to_string(),
                    old: change.old_index().map(|i| i + 1),
                    new: change.new_index().map(|i| i + 1),
                });
            }
        }
    }
    (out, adds, dels)
}

pub fn preview(tx: &Transaction, config_path: &Path, changes: &[Change]) -> AppResult<Preview> {
    let p = plan(tx, config_path, changes)?;
    let old = p.current.file.render();
    let (diff, additions, deletions) = diff_lines(&old, &p.new_text);
    Ok(Preview {
        config_path: config_path.display().to_string(),
        file_exists: p.current.bytes.is_some(),
        base_hash: p.current.hash,
        result_hash: sha256_hex(p.new_text.as_bytes()),
        diff,
        additions,
        deletions,
        confirmation_word: settings::confirmation_word(tx)?,
    })
}

pub struct ApplyRequest<'a> {
    pub changes: &'a [Change],
    pub base_hash: &'a str,
    pub result_hash: &'a str,
    pub confirmation: &'a str,
}

/// Runs every check [`apply`] makes, without writing. The command calls it before taking the
/// automatic backup, so a wrong word or stale preview doesn't create a backup.
pub fn check(tx: &Transaction, config_path: &Path, req: &ApplyRequest) -> AppResult<()> {
    checked_plan(tx, config_path, req).map(|_| ())
}

fn checked_plan(tx: &Transaction, config_path: &Path, req: &ApplyRequest) -> AppResult<Plan> {
    if req.confirmation != settings::confirmation_word(tx)? {
        return Err(AppError::Validation(
            "the confirmation word does not match".into(),
        ));
    }
    let p = plan(tx, config_path, req.changes)?;
    if p.current.hash != req.base_hash {
        return Err(AppError::Conflict(format!(
            "{} was changed outside RidgeSSH after the preview. Review the changes again.",
            config_path.display()
        )));
    }
    if sha256_hex(p.new_text.as_bytes()) != req.result_hash {
        return Err(AppError::Conflict(
            "the changes differ from the preview. Review them again.".into(),
        ));
    }
    Ok(p)
}

pub fn apply(
    tx: &Transaction,
    paths: &AppPaths,
    config_path: &Path,
    req: ApplyRequest,
) -> AppResult<ApplyResult> {
    let p = checked_plan(tx, config_path, &req)?;
    let previous_copy = match &p.current.bytes {
        Some(bytes) => Some(save_previous(&paths.data_dir, bytes)?),
        None => None,
    };
    atomic::replace_file(config_path, p.new_text.as_bytes())?;
    if std::fs::read(config_path)? != p.new_text.as_bytes() {
        return Err(AppError::Other(format!(
            "{} reads back differently than written. The previous version is in {}",
            config_path.display(),
            previous_copy
                .as_ref()
                .map_or("(no copy)".into(), |b| b.display().to_string())
        )));
    }
    tracing::info!(
        path = %config_path.display(),
        changes = p.changes.len(),
        previous = ?previous_copy,
        "ssh config written"
    );

    let ts = now();
    for (before, op) in &p.changes {
        match op {
            Op::Update | Op::Add => {
                let conn = before.connection();
                let fingerprint = ssh_config::find_host_block(&p.new_file, &conn.alias).map(|b| {
                    ssh_config::extract_host(&p.new_file, b, &conn.alias, &mut Vec::new())
                        .fingerprint
                });
                tx.execute(
                    "UPDATE hosts SET sync_state = 'synced', config_dirty = 0, config_snapshot = ?2,
                        config_fingerprint = ?3, config_source = NULL,
                        last_seen_in_config_at = ?4, removed_from_config_at = NULL
                     WHERE id = ?1",
                    params![before.id, serde_json::to_string(&conn)?, fingerprint, ts],
                )?;
            }
            Op::Remove => {
                tx.execute(
                    "UPDATE hosts SET sync_state = 'db_only', config_dirty = 0,
                        config_snapshot = NULL, config_fingerprint = NULL, config_source = NULL,
                        removed_from_config_at = NULL
                     WHERE id = ?1",
                    [&before.id],
                )?;
            }
        }
        let after = hosts::get(tx, &before.id)?;
        history::record(
            tx,
            Source::User,
            EntityType::Host,
            &before.id,
            Action::Update,
            Some(before),
            Some(&after),
        )?;
    }

    // Reconcile everything else against the file as now written (normally a no-op).
    let report = sync::run(tx, &ssh_config::load(config_path)?, config_path)?;
    Ok(ApplyResult {
        config_path: config_path.display().to_string(),
        previous_copy: previous_copy.map(|b| b.display().to_string()),
        auto_backup: None,
        changed: p.changes.len(),
        report,
    })
}

/// Keeps a plain copy of a config about to be replaced in `<data dir>/config-history/` (local,
/// like the database; the backup folder may be a cloud folder and only holds encrypted files).
pub fn save_previous(data_dir: &Path, bytes: &[u8]) -> AppResult<PathBuf> {
    let dir = data_dir.join("config-history");
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
    let path = dir.join(format!("config-{stamp}"));
    std::fs::write(&path, bytes)?;

    let mut existing: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("config-"))
        })
        .collect();
    existing.sort();
    let excess = existing.len().saturating_sub(KEEP_BACKUPS);
    for old in &existing[..excess] {
        let _ = std::fs::remove_file(old);
    }
    Ok(path)
}

#[cfg(test)]
mod tests;
