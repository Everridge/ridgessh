//! Export and import of hosts, groups and scripts.

use std::path::PathBuf;

use age::secrecy::SecretString;
use tauri::State;

use crate::AppState;
use crate::backup::{self, Reason, password};
use crate::config_write::atomic;
use crate::error::{AppError, AppResult};
use crate::transfer::{
    self, ExportSummary, ImportPreview, ImportRequest, ImportResult, ImportSource, Selection,
};

/// The user's home folder: where `~` in key paths points and whose `.ssh` keys go into.
fn home(state: &AppState) -> AppResult<PathBuf> {
    state
        .paths
        .ssh_config
        .parent()
        .and_then(|ssh| ssh.parent())
        .map(PathBuf::from)
        .ok_or_else(|| AppError::Other("cannot resolve the home folder".into()))
}

#[tauri::command]
pub async fn export_create(
    state: State<'_, AppState>,
    path: String,
    password: String,
    selection: Selection,
) -> AppResult<ExportSummary> {
    password::check_strength(&password)?;
    let path = transfer::export_path(&path, transfer::EXTENSION)?;
    let home = home(&state)?;
    let (doc, warnings) = state.db.tx(|tx| transfer::build(tx, &selection, &home))?;
    transfer::write_export(
        &path,
        &doc,
        &SecretString::from(password),
        backup::WORK_FACTOR,
    )?;
    tracing::info!(path = %path.display(), hosts = doc.hosts.len(), "export written");
    Ok(ExportSummary {
        path: path.display().to_string(),
        hosts: doc.hosts.len(),
        groups: doc.groups.len(),
        scripts: doc.scripts.len(),
        keys: doc.keys.len(),
        warnings,
    })
}

/// Writes the selected hosts as a plain (unencrypted) ssh_config file. Returns its path.
#[tauri::command]
pub fn export_config_file(
    state: State<'_, AppState>,
    path: String,
    host_ids: Vec<String>,
) -> AppResult<String> {
    // Any file name is fine for a config file; only the path must be absolute.
    let path = PathBuf::from(path.trim());
    if !path.is_absolute() {
        return Err(AppError::Validation(format!(
            "{} is not a valid file path",
            path.display()
        )));
    }
    let text = state.db.tx(|tx| transfer::config_snippet(tx, &host_ids))?;
    let same = |a: &std::path::Path, b: &std::path::Path| {
        a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
    };
    // Never the live config or a file it includes: those change only through the guarded
    // writer.
    let mut live = vec![state.paths.ssh_config.clone()];
    if let Ok(parsed) = crate::ssh_config::load(&state.paths.ssh_config) {
        live.extend(parsed.files.into_iter().map(|f| f.path));
    }
    if live.iter().any(|p| same(&path, p)) {
        return Err(AppError::Validation(
            "choose another file: your ssh config files are only written through \"Write to config\"".into(),
        ));
    }
    atomic::replace_file(&path, text.as_bytes())?;
    Ok(path.display().to_string())
}

/// Decrypts / parses an import source and compares it with the database. Read-only.
#[tauri::command]
pub async fn import_preview(
    state: State<'_, AppState>,
    source: ImportSource,
) -> AppResult<ImportPreview> {
    let loaded = transfer::load(&source)?;
    let home = home(&state)?;
    state.db.tx(|tx| transfer::preview(tx, &loaded, &home))
}

#[tauri::command]
pub async fn import_apply(
    state: State<'_, AppState>,
    source: ImportSource,
    request: ImportRequest,
) -> AppResult<ImportResult> {
    let loaded = transfer::load(&source)?;
    let auto_backup = state.auto_backup(Reason::Import)?;
    let mut result = state.db.tx(|tx| transfer::apply(tx, &loaded, &request))?;
    if request.install_keys {
        let (installed, warnings) = transfer::keys::install(&loaded.doc.keys, &home(&state)?);
        result.keys_installed = installed;
        result.warnings.extend(warnings);
    }
    result.auto_backup = auto_backup;
    tracing::info!(
        created = result.created,
        overwritten = result.overwritten,
        copied = result.copied,
        "import applied"
    );
    Ok(result)
}
