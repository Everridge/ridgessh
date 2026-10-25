//! Export / import (M8).
//!
//! An export is one `.rsexport` file: a JSON document (hosts, the groups they sit in, scripts
//! and, only when asked for, the key files hosts name in `IdentityFile`) encrypted with an
//! `age` passphrase like backups, so `age -d` opens it without RidgeSSH. Its password is asked
//! each time and never stored.
//!
//! Import never writes ~/.ssh/config. Imported connection values land in the database, so new
//! hosts and changed connections show up as pending changes for the guarded writer. A plain
//! ssh_config file from another machine goes through the same preview and merge.
//!
//! Key files are written only when asked, only into ~/.ssh and never over an existing file.

pub mod keys;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use age::secrecy::SecretString;
use rusqlite::Transaction;
use serde::{Deserialize, Serialize};

use crate::backup;
use crate::config_write::{self, atomic};
use crate::db::groups::{self, GroupInput};
use crate::db::hosts::{self, Directive, Host, HostField, HostInput, UserMatch};
use crate::db::tags::Tag;
use crate::error::{AppError, AppResult};
use crate::history::Source;
use crate::scripts::{self, RunMode, Script, ScriptInput};
use crate::ssh_config::{self, ConfigFile, writer};
pub use keys::KeyPreview;
#[cfg(test)]
use keys::KeyStatus;

pub const EXTENSION: &str = "rsexport";
const FORMAT: u32 = 1;
const KIND: &str = "ridgessh-export";
/// Exports are small (text); anything bigger is not one.
const MAX_EXPORT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportGroup {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub color: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportHost {
    pub id: String,
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    pub proxy_jump: Option<String>,
    pub extra_options: Vec<Directive>,
    /// Per-user `Match` blocks.
    pub user_matches: Vec<UserMatch>,
    pub group_id: Option<String>,
    pub description: String,
    pub notes: String,
    pub color: Option<String>,
    pub favorite: bool,
    pub tags: Vec<String>,
    pub fields: Vec<HostField>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportScript {
    pub id: String,
    pub name: String,
    pub description: String,
    pub content: String,
    pub interpreter: String,
    pub default_args: String,
    pub run_mode: RunMode,
}

/// A private key (and its `.pub`, if there was one), base64-encoded.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportKey {
    /// The `IdentityFile` value as hosts name it, e.g. `~/.ssh/id_work`.
    pub identity_file: String,
    pub private_key: String,
    pub public_key: Option<String>,
}

/// The decrypted contents of an `.rsexport` file.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Document {
    pub format: u32,
    pub kind: String,
    pub app_version: String,
    pub created_at: String,
    pub groups: Vec<ExportGroup>,
    pub hosts: Vec<ExportHost>,
    pub scripts: Vec<ExportScript>,
    pub keys: Vec<ExportKey>,
}

// ---------------------------------------------------------------------------------------
// Export

/// What to export. Hosts bring their groups (with parents) along.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Selection {
    pub host_ids: Vec<String>,
    pub script_ids: Vec<String>,
    /// Include the private key files hosts name in `IdentityFile`.
    pub include_keys: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub path: String,
    pub hosts: usize,
    pub groups: usize,
    pub scripts: usize,
    pub keys: usize,
    /// Keys that could not be included, and similar.
    pub warnings: Vec<String>,
}

fn export_host(h: &Host) -> ExportHost {
    ExportHost {
        id: h.id.clone(),
        alias: h.alias.clone(),
        hostname: h.hostname.clone(),
        user: h.user.clone(),
        port: h.port,
        identity_file: h.identity_file.clone(),
        proxy_jump: h.proxy_jump.clone(),
        extra_options: h.extra_options.clone(),
        user_matches: h.user_matches.clone(),
        group_id: h.group_id.clone(),
        description: h.description.clone(),
        notes: h.notes.clone(),
        color: h.color.clone(),
        favorite: h.favorite,
        tags: h.tags.iter().map(|t| t.name.clone()).collect(),
        fields: h.fields.clone(),
    }
}

fn export_script(s: Script) -> ExportScript {
    ExportScript {
        id: s.id,
        name: s.name,
        description: s.description,
        content: s.content,
        interpreter: s.interpreter,
        default_args: s.default_args,
        run_mode: s.run_mode,
    }
}

/// Live hosts with the given ids, in list order. Unknown or trashed ids are an error.
fn selected_hosts(tx: &Transaction, ids: &[String]) -> AppResult<Vec<Host>> {
    let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let found: Vec<Host> = hosts::list(tx)?
        .into_iter()
        .filter(|h| h.deleted_at.is_none() && wanted.contains(h.id.as_str()))
        .collect();
    if found.len() != wanted.len() {
        return Err(AppError::Conflict(
            "some selected hosts no longer exist; select them again".into(),
        ));
    }
    Ok(found)
}

/// Builds the export document. `home` resolves `~` in key paths.
pub fn build(tx: &Transaction, sel: &Selection, home: &Path) -> AppResult<(Document, Vec<String>)> {
    if sel.host_ids.is_empty() && sel.script_ids.is_empty() {
        return Err(AppError::Validation("select something to export".into()));
    }
    let hosts = selected_hosts(tx, &sel.host_ids)?;

    // The hosts' groups and all their parents, so the tree can be rebuilt.
    let all_groups: HashMap<String, groups::Group> = groups::list(tx)?
        .into_iter()
        .map(|g| (g.id.clone(), g))
        .collect();
    let mut wanted: HashSet<String> = HashSet::new();
    for h in &hosts {
        let mut cursor = h.group_id.clone();
        while let Some(id) = cursor {
            if !wanted.insert(id.clone()) {
                break;
            }
            cursor = all_groups.get(&id).and_then(|g| g.parent_id.clone());
        }
    }
    let mut groups: Vec<ExportGroup> = all_groups
        .values()
        .filter(|g| wanted.contains(&g.id))
        .map(|g| ExportGroup {
            id: g.id.clone(),
            parent_id: g.parent_id.clone(),
            name: g.name.clone(),
            color: g.color.clone(),
            icon: g.icon.clone(),
        })
        .collect();
    groups.sort_by_key(|g| g.name.to_lowercase());

    let mut scripts_out = Vec::new();
    for id in &sel.script_ids {
        scripts_out.push(export_script(scripts::get(tx, id)?));
    }

    let mut warnings = Vec::new();
    let keys = if sel.include_keys {
        keys::collect(&hosts, home, &mut warnings)
    } else {
        Vec::new()
    };

    let doc = Document {
        format: FORMAT,
        kind: KIND.into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        created_at: crate::db::now(),
        groups,
        hosts: hosts.iter().map(export_host).collect(),
        scripts: scripts_out,
        keys,
    };
    Ok((doc, warnings))
}

/// The path chosen in the save dialog, with the right extension.
pub fn export_path(path: &str, extension: &str) -> AppResult<PathBuf> {
    let mut p = PathBuf::from(path.trim());
    if !p.is_absolute() || path.chars().any(char::is_control) {
        return Err(AppError::Validation(format!(
            "{path} is not a valid file path"
        )));
    }
    if p.extension()
        .is_none_or(|e| !e.eq_ignore_ascii_case(extension))
    {
        let mut name = p.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".{extension}"));
        p.set_file_name(name);
    }
    Ok(p)
}

/// Encrypts `doc` and writes it to `path` (replacing an existing file atomically).
pub fn write_export(
    path: &Path,
    doc: &Document,
    password: &SecretString,
    work_factor: u8,
) -> AppResult<()> {
    let plain = serde_json::to_vec_pretty(doc)?;
    let encrypted = backup::encrypt(&plain, password, work_factor)?;
    Ok(atomic::replace_file(path, &encrypted)?)
}

/// Selected hosts as a plain ssh_config text (unencrypted; connection values only).
pub fn config_snippet(tx: &Transaction, host_ids: &[String]) -> AppResult<String> {
    let hosts = selected_hosts(tx, host_ids)?;
    if hosts.is_empty() {
        return Err(AppError::Validation("select hosts to export".into()));
    }
    let header = format!(
        "# Exported from RidgeSSH on {} ({} host{})\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
        hosts.len(),
        if hosts.len() == 1 { "" } else { "s" }
    );
    let file = ConfigFile::parse("export", &header);
    let edits: Vec<_> = hosts
        .iter()
        .map(|h| writer::Edit::Add {
            conn: h.connection(),
        })
        .collect();
    let text = writer::apply(&file, &edits, &HashSet::new())?;
    writer::verify(&file, &text, &edits)?;
    Ok(text)
}

// ---------------------------------------------------------------------------------------
// Import: reading the source

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportSource {
    /// A RidgeSSH `.rsexport` file.
    Export { path: String, password: String },
    /// A plain ssh_config file (e.g. from another machine).
    Config { path: String },
}

/// A decrypted / parsed import source.
pub struct Loaded {
    pub kind: &'static str,
    pub file_name: String,
    pub doc: Document,
    pub warnings: Vec<String>,
}

fn read_limited(path: &Path, max: u64, what: &str) -> AppResult<Vec<u8>> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() || meta.len() > max {
        return Err(AppError::Validation(format!(
            "{} is not a {what}",
            path.display()
        )));
    }
    Ok(std::fs::read(path)?)
}

pub fn load(source: &ImportSource) -> AppResult<Loaded> {
    let (path, mut loaded) = match source {
        ImportSource::Export { path, password } => {
            let path = PathBuf::from(path);
            let data = read_limited(&path, MAX_EXPORT_BYTES, "RidgeSSH export")?;
            let plain = backup::decrypt(&data, &SecretString::from(password.clone()), "export")?;
            let doc: Document = serde_json::from_slice(&plain)
                .map_err(|e| AppError::Validation(format!("the export file is damaged: {e}")))?;
            if doc.kind != KIND {
                return Err(AppError::Validation(
                    "this is not a RidgeSSH export (a backup is restored from Backups)".into(),
                ));
            }
            if doc.format > FORMAT {
                return Err(AppError::Validation(
                    "this export was made by a newer RidgeSSH version".into(),
                ));
            }
            (
                path,
                Loaded {
                    kind: "export",
                    file_name: String::new(),
                    doc,
                    warnings: Vec::new(),
                },
            )
        }
        ImportSource::Config { path } => {
            let path = PathBuf::from(path);
            let bytes = read_limited(&path, MAX_CONFIG_BYTES, "ssh config file")?;
            let text = ssh_config::decode(&path, bytes)?;
            (path.clone(), from_config(&path, &text))
        }
    };
    loaded.file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dedupe(&mut loaded);
    Ok(loaded)
}

/// The concrete `Host` blocks of a plain config file, as an import document.
pub fn from_config(path: &Path, text: &str) -> Loaded {
    let parsed = ssh_config::parse_single(path, text);
    let mut warnings = parsed.warnings;
    if !parsed.rules.is_empty() {
        warnings.push(format!(
            "{} wildcard, Match or duplicate block{} cannot be imported: {}",
            parsed.rules.len(),
            if parsed.rules.len() == 1 { "" } else { "s" },
            parsed
                .rules
                .iter()
                .map(|r| if r.kind == "match" {
                    format!("Match {}", r.header)
                } else {
                    format!("Host {}", r.header)
                })
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    let hosts = parsed
        .hosts
        .into_iter()
        .map(|h| ExportHost {
            id: format!("config:{}", h.alias),
            alias: h.alias,
            hostname: h.hostname,
            user: h.user,
            port: h.port,
            identity_file: h.identity_file,
            proxy_jump: h.proxy_jump,
            extra_options: h.extra_options,
            user_matches: h.user_matches,
            ..Default::default()
        })
        .collect();
    Loaded {
        kind: "config",
        file_name: String::new(),
        doc: Document {
            format: FORMAT,
            kind: KIND.into(),
            hosts,
            ..Default::default()
        },
        warnings,
    }
}

/// Drops repeated ids and aliases (a hand-edited file), keeping the first.
fn dedupe(l: &mut Loaded) {
    let mut ids = HashSet::new();
    let mut aliases = HashSet::new();
    let warnings = &mut l.warnings;
    l.doc.hosts.retain(|h| {
        let ok = ids.insert(h.id.clone()) && aliases.insert(h.alias.clone());
        if !ok {
            warnings.push(format!(
                "Host {} appears twice; the second one is skipped",
                h.alias
            ));
        }
        ok
    });
    let mut ids = HashSet::new();
    l.doc.scripts.retain(|s| ids.insert(s.id.clone()));
}

// ---------------------------------------------------------------------------------------
// Import: preview

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    /// Nothing here by that name.
    New,
    /// Already here with the same values.
    Same,
    /// Here, with different values.
    Differs,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Skip,
    /// Add a new item (only for `New`).
    Import,
    /// Replace the existing item's values (it keeps its group).
    Overwrite,
    /// Add it next to the existing one under a new name.
    Copy,
}

impl ItemStatus {
    fn allows(self, s: Strategy) -> bool {
        match self {
            ItemStatus::New => matches!(s, Strategy::Skip | Strategy::Import),
            ItemStatus::Same | ItemStatus::Differs => {
                matches!(s, Strategy::Skip | Strategy::Overwrite | Strategy::Copy)
            }
        }
    }

    fn suggested(self) -> Strategy {
        match self {
            ItemStatus::New => Strategy::Import,
            _ => Strategy::Skip,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostPreview {
    pub key: String,
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    /// Group names from the top, as in the file.
    pub group_path: Vec<String>,
    pub status: ItemStatus,
    pub existing_id: Option<String>,
    /// What differs from the existing host (labels).
    pub differences: Vec<String>,
    /// The alias "import as copy" would use.
    pub copy_alias: String,
    pub suggested: Strategy,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptPreview {
    pub key: String,
    pub name: String,
    pub status: ItemStatus,
    pub existing_id: Option<String>,
    pub differences: Vec<String>,
    pub copy_name: String,
    pub suggested: Strategy,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    /// `export` or `config`.
    pub kind: &'static str,
    pub file_name: String,
    pub created_at: Option<String>,
    pub app_version: Option<String>,
    pub hosts: Vec<HostPreview>,
    pub scripts: Vec<ScriptPreview>,
    pub keys: Vec<KeyPreview>,
    pub warnings: Vec<String>,
}

fn tag_set(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = names.map(|n| n.trim().to_lowercase()).collect();
    v.sort();
    v.dedup();
    v
}

fn host_differences(h: &Host, x: &ExportHost) -> Vec<String> {
    let opt = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let mut d = Vec::new();
    let mut check = |label: &str, differs: bool| {
        if differs {
            d.push(label.to_string());
        }
    };
    check("hostname", h.hostname != opt(&x.hostname));
    check("user", h.user != opt(&x.user));
    check("port", h.port != x.port);
    check("identity file", h.identity_file != opt(&x.identity_file));
    check("proxy jump", h.proxy_jump != opt(&x.proxy_jump));
    check("options", h.extra_options != x.extra_options);
    check("users", h.user_matches != x.user_matches);
    check("description", h.description != x.description.trim());
    check("notes", h.notes != x.notes);
    check("color", h.color != opt(&x.color));
    check("favorite", h.favorite != x.favorite);
    check(
        "tags",
        tag_set(h.tags.iter().map(|t: &Tag| t.name.clone())) != tag_set(x.tags.iter().cloned()),
    );
    check("custom fields", h.fields != x.fields);
    d
}

const CONNECTION_LABELS: [&str; 7] = [
    "hostname",
    "user",
    "port",
    "identity file",
    "proxy jump",
    "options",
    "users",
];

fn script_differences(s: &Script, x: &ExportScript) -> Vec<String> {
    let mut d = Vec::new();
    if s.content != scripts::normalize(&x.content) {
        d.push("content".into());
    }
    if s.description != x.description {
        d.push("description".into());
    }
    if s.interpreter != x.interpreter.trim() {
        d.push("interpreter".into());
    }
    if s.default_args != x.default_args.trim() {
        d.push("arguments".into());
    }
    if s.run_mode != x.run_mode {
        d.push("run mode".into());
    }
    d
}

/// `alias-2`, `alias-3`, … whichever is free first.
fn free_alias(alias: &str, taken: &HashSet<String>) -> String {
    (2..)
        .map(|n| format!("{alias}-{n}"))
        .find(|a| !taken.contains(a))
        .unwrap_or_default()
}

fn group_path(doc: &Document, group_id: Option<&str>) -> Vec<String> {
    let by_id: HashMap<&str, &ExportGroup> =
        doc.groups.iter().map(|g| (g.id.as_str(), g)).collect();
    let mut path = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = group_id;
    while let Some(id) = cursor {
        let Some(g) = by_id.get(id) else { break };
        if !seen.insert(id) {
            break;
        }
        path.push(g.name.clone());
        cursor = g.parent_id.as_deref();
    }
    path.reverse();
    path
}

struct Matched {
    hosts: Vec<(HostPreview, Option<Host>)>,
    scripts: Vec<(ScriptPreview, Option<Script>)>,
}

fn match_items(tx: &Transaction, doc: &Document) -> AppResult<Matched> {
    let live: Vec<Host> = hosts::list(tx)?
        .into_iter()
        .filter(|h| h.deleted_at.is_none())
        .collect();
    let mut taken: HashSet<String> = live.iter().map(|h| h.alias.clone()).collect();
    taken.extend(doc.hosts.iter().map(|h| h.alias.clone()));
    let by_alias: HashMap<&str, &Host> = live.iter().map(|h| (h.alias.as_str(), h)).collect();

    let mut out_hosts = Vec::new();
    for x in &doc.hosts {
        let existing = by_alias.get(x.alias.as_str()).copied();
        let differences = existing.map(|h| host_differences(h, x)).unwrap_or_default();
        let status = match existing {
            None => ItemStatus::New,
            Some(_) if differences.is_empty() => ItemStatus::Same,
            Some(_) => ItemStatus::Differs,
        };
        let connection_differs = differences
            .iter()
            .any(|d| CONNECTION_LABELS.contains(&d.as_str()));
        let note = existing.and_then(|h| {
            (connection_differs && h.config_source.is_some()).then(|| {
                "Defined in an included config file: overwritten connection values cannot be written there.".to_string()
            })
        });
        let copy_alias = free_alias(&x.alias, &taken);
        taken.insert(copy_alias.clone());
        out_hosts.push((
            HostPreview {
                key: x.id.clone(),
                alias: x.alias.clone(),
                hostname: x.hostname.clone(),
                user: x.user.clone(),
                port: x.port,
                group_path: group_path(doc, x.group_id.as_deref()),
                status,
                existing_id: existing.map(|h| h.id.clone()),
                differences,
                copy_alias,
                suggested: status.suggested(),
                note,
            },
            existing.cloned(),
        ));
    }

    let local_scripts = scripts::list(tx)?;
    let mut out_scripts = Vec::new();
    for x in &doc.scripts {
        let existing = local_scripts
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(x.name.trim()));
        let differences = existing
            .map(|s| script_differences(s, x))
            .unwrap_or_default();
        let status = match existing {
            None => ItemStatus::New,
            Some(_) if differences.is_empty() => ItemStatus::Same,
            Some(_) => ItemStatus::Differs,
        };
        out_scripts.push((
            ScriptPreview {
                key: x.id.clone(),
                name: x.name.clone(),
                status,
                existing_id: existing.map(|s| s.id.clone()),
                differences,
                copy_name: format!("{} (imported)", x.name.trim()),
                suggested: status.suggested(),
            },
            existing.cloned(),
        ));
    }
    Ok(Matched {
        hosts: out_hosts,
        scripts: out_scripts,
    })
}

/// What importing `loaded` would do, item by item. Read-only.
pub fn preview(tx: &Transaction, loaded: &Loaded, home: &Path) -> AppResult<ImportPreview> {
    let m = match_items(tx, &loaded.doc)?;
    let is_export = loaded.kind == "export";
    Ok(ImportPreview {
        kind: loaded.kind,
        file_name: loaded.file_name.clone(),
        created_at: is_export.then(|| loaded.doc.created_at.clone()),
        app_version: is_export.then(|| loaded.doc.app_version.clone()),
        hosts: m.hosts.into_iter().map(|(p, _)| p).collect(),
        scripts: m.scripts.into_iter().map(|(p, _)| p).collect(),
        keys: keys::preview(&loaded.doc.keys, home),
        warnings: loaded.warnings.clone(),
    })
}

// ---------------------------------------------------------------------------------------
// Import: apply

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub key: String,
    pub strategy: Strategy,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportRequest {
    /// Imported groups go under this group (top level when `None`).
    pub target_group_id: Option<String>,
    /// Items not listed are skipped.
    pub hosts: Vec<Choice>,
    pub scripts: Vec<Choice>,
    pub install_keys: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub created: usize,
    pub overwritten: usize,
    pub copied: usize,
    pub skipped: usize,
    pub scripts: usize,
    pub groups_created: usize,
    pub keys_installed: Vec<String>,
    /// Imported hosts with connection values to write to ~/.ssh/config.
    pub pending_host_ids: Vec<String>,
    pub warnings: Vec<String>,
    pub auto_backup: Option<backup::BackupInfo>,
}

fn host_input(x: &ExportHost, alias: &str, group_id: Option<String>) -> HostInput {
    HostInput {
        alias: alias.to_string(),
        hostname: x.hostname.clone(),
        user: x.user.clone(),
        port: x.port,
        identity_file: x.identity_file.clone(),
        proxy_jump: x.proxy_jump.clone(),
        extra_options: x.extra_options.clone(),
        user_matches: x.user_matches.clone(),
        group_id,
        description: x.description.clone(),
        notes: x.notes.clone(),
        color: x.color.clone(),
        favorite: x.favorite,
        tags: x.tags.clone(),
        fields: x.fields.clone(),
    }
}

fn script_input(x: &ExportScript, name: &str) -> ScriptInput {
    ScriptInput {
        name: name.to_string(),
        description: x.description.clone(),
        content: x.content.clone(),
        interpreter: if x.interpreter.trim().is_empty() {
            "bash".into()
        } else {
            x.interpreter.clone()
        },
        default_args: x.default_args.clone(),
        run_mode: x.run_mode,
    }
}

/// Maps the file's groups onto local ones: same name under the same parent is reused,
/// anything else is created. Created lazily, so only groups that receive a host appear.
struct GroupMapper<'a> {
    by_id: HashMap<&'a str, &'a ExportGroup>,
    target: Option<String>,
    mapped: HashMap<String, Option<String>>,
    created: usize,
}

impl<'a> GroupMapper<'a> {
    fn new(doc: &'a Document, target: Option<String>) -> Self {
        Self {
            by_id: doc.groups.iter().map(|g| (g.id.as_str(), g)).collect(),
            target,
            mapped: HashMap::new(),
            created: 0,
        }
    }

    fn resolve(&mut self, tx: &Transaction, id: Option<&str>) -> AppResult<Option<String>> {
        // Walk up to the top (or a group mapped already), then create downwards.
        let mut chain: Vec<&ExportGroup> = Vec::new();
        let mut cursor = id;
        let mut parent = self.target.clone();
        while let Some(gid) = cursor {
            if let Some(local) = self.mapped.get(gid) {
                parent = local.clone();
                break;
            }
            let Some(g) = self.by_id.get(gid).copied() else {
                break; // unknown group: put the host at the target
            };
            if chain.iter().any(|c| c.id == g.id) {
                break; // a cycle in a hand-edited file
            }
            chain.push(g);
            cursor = g.parent_id.as_deref();
        }
        for g in chain.into_iter().rev() {
            let name = g.name.trim();
            let existing = groups::list(tx)?
                .into_iter()
                .find(|l| l.parent_id == parent && l.name.trim().eq_ignore_ascii_case(name));
            let local = match existing {
                Some(l) => l.id,
                None => {
                    self.created += 1;
                    groups::create(
                        tx,
                        &GroupInput {
                            name: name.to_string(),
                            parent_id: parent.clone(),
                            color: g.color.clone(),
                            icon: g.icon.clone(),
                        },
                        Source::Import,
                    )?
                    .id
                }
            };
            self.mapped.insert(g.id.clone(), Some(local.clone()));
            parent = Some(local);
        }
        Ok(parent)
    }
}

fn changed_since_preview(what: &str) -> AppError {
    AppError::Conflict(format!(
        "{what} changed since the preview. Review the import again."
    ))
}

/// Imports the chosen items in the caller's transaction (one history batch). Key files are
/// installed separately, after the transaction commits ([`keys::install`]).
pub fn apply(tx: &Transaction, loaded: &Loaded, req: &ImportRequest) -> AppResult<ImportResult> {
    let doc = &loaded.doc;
    if let Some(g) = &req.target_group_id {
        groups::get(tx, g)?;
    }
    let m = match_items(tx, doc)?;
    let host_choice: HashMap<&str, Strategy> = req
        .hosts
        .iter()
        .map(|c| (c.key.as_str(), c.strategy))
        .collect();
    let script_choice: HashMap<&str, Strategy> = req
        .scripts
        .iter()
        .map(|c| (c.key.as_str(), c.strategy))
        .collect();
    if host_choice
        .keys()
        .any(|k| !doc.hosts.iter().any(|h| h.id == *k))
        || script_choice
            .keys()
            .any(|k| !doc.scripts.iter().any(|s| s.id == *k))
    {
        return Err(AppError::Validation("unknown item in the import".into()));
    }

    // The file's own warnings were shown in the preview.
    let mut result = ImportResult::default();
    let mut mapper = GroupMapper::new(doc, req.target_group_id.clone());
    let mut touched = Vec::new();

    for ((p, existing), x) in m.hosts.iter().zip(&doc.hosts) {
        let strategy = host_choice
            .get(p.key.as_str())
            .copied()
            .unwrap_or(Strategy::Skip);
        if !p.status.allows(strategy) {
            return Err(changed_since_preview(&format!("Host {}", p.alias)));
        }
        let with_alias = |e: AppError| AppError::Validation(format!("Host {}: {e}", x.alias));
        let id = match strategy {
            Strategy::Skip => {
                result.skipped += 1;
                continue;
            }
            Strategy::Import | Strategy::Copy => {
                let group = mapper.resolve(tx, x.group_id.as_deref())?;
                let alias = if strategy == Strategy::Copy {
                    result.copied += 1;
                    &p.copy_alias
                } else {
                    result.created += 1;
                    &x.alias
                };
                hosts::create(tx, host_input(x, alias, group), Source::Import)
                    .map_err(with_alias)?
                    .id
            }
            Strategy::Overwrite => {
                let h = existing
                    .as_ref()
                    .ok_or_else(|| changed_since_preview(&format!("Host {}", p.alias)))?;
                result.overwritten += 1;
                hosts::update(
                    tx,
                    &h.id,
                    host_input(x, &h.alias, h.group_id.clone()),
                    Source::Import,
                )
                .map_err(with_alias)?
                .id
            }
        };
        touched.push(id);
    }

    for ((p, existing), x) in m.scripts.iter().zip(&doc.scripts) {
        let strategy = script_choice
            .get(p.key.as_str())
            .copied()
            .unwrap_or(Strategy::Skip);
        if !p.status.allows(strategy) {
            return Err(changed_since_preview(&format!("Script {}", p.name)));
        }
        let with_name = |e: AppError| AppError::Validation(format!("Script {}: {e}", x.name));
        match strategy {
            Strategy::Skip => continue,
            Strategy::Import => {
                scripts::create(tx, script_input(x, &x.name), Source::Import).map_err(with_name)?;
            }
            Strategy::Copy => {
                scripts::create(tx, script_input(x, &p.copy_name), Source::Import)
                    .map_err(with_name)?;
            }
            Strategy::Overwrite => {
                let s = existing
                    .as_ref()
                    .ok_or_else(|| changed_since_preview(&format!("Script {}", p.name)))?;
                scripts::update(tx, &s.id, script_input(x, &s.name), Source::Import)
                    .map_err(with_name)?;
            }
        }
        result.scripts += 1;
    }

    result.groups_created = mapper.created;
    for id in touched {
        if config_write::offered_op(&hosts::get(tx, &id)?).is_some() {
            result.pending_host_ids.push(id);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
