//! Private key files in exports (opt-in).
//!
//! Only files that hosts name in `IdentityFile` are exported. On import a key is written
//! only into `~/.ssh`, only when nothing exists at that path, and never replaces a file.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use base64::Engine;
use serde::Serialize;

use super::ExportKey;
use crate::db::hosts::Host;

/// Private keys are a few KB; anything bigger is not one.
const MAX_KEY_BYTES: u64 = 64 * 1024;

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// Where an `IdentityFile` value points on this machine. `None` for values that depend on
/// ssh's runtime tokens (`%d`, `${VAR}`) or are relative.
pub fn resolve(identity_file: &str, home: &Path) -> Option<PathBuf> {
    let v = identity_file.trim().trim_matches('"');
    if v.contains('%') || v.contains("${") {
        return None;
    }
    if let Some(rest) = v.strip_prefix("~/").or_else(|| v.strip_prefix("~\\")) {
        // Join segment by segment so the result uses this platform's separators.
        return Some(
            rest.split(['/', '\\'])
                .filter(|s| !s.is_empty())
                .fold(home.to_path_buf(), |p, s| p.join(s)),
        );
    }
    let p = PathBuf::from(v);
    p.is_absolute().then_some(p)
}

fn read_key(path: &Path) -> Result<Vec<u8>, String> {
    let meta = std::fs::metadata(path).map_err(|_| "not found".to_string())?;
    if !meta.is_file() || meta.len() > MAX_KEY_BYTES {
        return Err("not a key file".into());
    }
    std::fs::read(path).map_err(|e| e.to_string())
}

/// The key files of `hosts`. Problems become warnings; the export goes ahead without them.
pub fn collect(hosts: &[Host], home: &Path, warnings: &mut Vec<String>) -> Vec<ExportKey> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    // The host's own key, then those of its per-user blocks.
    let named = hosts.iter().flat_map(|h| {
        h.identity_file
            .iter()
            .chain(
                h.user_matches
                    .iter()
                    .filter_map(|m| m.identity_file.as_ref()),
            )
            .map(move |v| (h, v.as_str()))
    });
    for (h, value) in named {
        if !seen.insert(value.to_string()) {
            continue;
        }
        let Some(path) = resolve(value, home) else {
            warnings.push(format!(
                "Key {value} ({}) was not included: only absolute and ~/ paths are supported",
                h.alias
            ));
            continue;
        };
        match read_key(&path) {
            Ok(private) => {
                let mut pub_path = path.clone().into_os_string();
                pub_path.push(".pub");
                out.push(ExportKey {
                    identity_file: value.to_string(),
                    private_key: b64().encode(private),
                    public_key: read_key(Path::new(&pub_path)).ok().map(|p| b64().encode(p)),
                });
            }
            Err(e) => warnings.push(format!("Key {value} ({}) was not included: {e}", h.alias)),
        }
    }
    out
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KeyStatus {
    /// Nothing at the target path: can be installed.
    New,
    /// The same key is already there.
    Same,
    /// A different file is there; it is never replaced.
    Exists,
    /// The path is outside ~/.ssh or cannot be resolved.
    Unsupported,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyPreview {
    pub identity_file: String,
    pub target: Option<String>,
    pub status: KeyStatus,
    pub has_public_key: bool,
}

/// `path` lies inside `<home>/.ssh` (lexically, no `..`).
fn inside_ssh_dir(path: &Path, home: &Path) -> bool {
    !path.components().any(|c| matches!(c, Component::ParentDir))
        && path.starts_with(home.join(".ssh"))
        && path != home.join(".ssh")
}

fn status(k: &ExportKey, home: &Path) -> (Option<PathBuf>, KeyStatus) {
    let Some(target) = resolve(&k.identity_file, home).filter(|p| inside_ssh_dir(p, home)) else {
        return (None, KeyStatus::Unsupported);
    };
    let st = match std::fs::read(&target) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => KeyStatus::New,
        Ok(bytes) if b64().decode(&k.private_key).is_ok_and(|d| d == bytes) => KeyStatus::Same,
        _ => KeyStatus::Exists,
    };
    (Some(target), st)
}

pub fn preview(keys: &[ExportKey], home: &Path) -> Vec<KeyPreview> {
    keys.iter()
        .map(|k| {
            let (target, status) = status(k, home);
            KeyPreview {
                identity_file: k.identity_file.clone(),
                target: target.map(|p| p.display().to_string()),
                status,
                has_public_key: k.public_key.is_some(),
            }
        })
        .collect()
}

/// Writes a new file; fails if anything already exists there.
fn write_new(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(data)?;
    f.sync_all()
}

/// Installs the keys whose target is free. Returns the paths written and warnings.
pub fn install(keys: &[ExportKey], home: &Path) -> (Vec<String>, Vec<String>) {
    let (mut installed, mut warnings) = (Vec::new(), Vec::new());
    for k in keys {
        let (target, st) = status(k, home);
        let Some(target) = target.filter(|_| st == KeyStatus::New) else {
            if st == KeyStatus::Exists {
                warnings.push(format!(
                    "{} already exists and was left unchanged",
                    k.identity_file
                ));
            }
            continue;
        };
        let Ok(private) = b64().decode(&k.private_key) else {
            warnings.push(format!("{}: damaged key data", k.identity_file));
            continue;
        };
        if let Err(e) = write_new(&target, &private) {
            warnings.push(format!("{}: {e}", target.display()));
            continue;
        }
        installed.push(target.display().to_string());
        if let Some(public) = k.public_key.as_deref().and_then(|p| b64().decode(p).ok()) {
            let mut pub_path = target.clone().into_os_string();
            pub_path.push(".pub");
            // An existing .pub is left alone too.
            let _ = write_new(Path::new(&pub_path), &public);
        }
    }
    (installed, warnings)
}
