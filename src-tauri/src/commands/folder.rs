//! Folder transfers (M11) in a terminal tab.

use std::path::Path;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use super::terminal::live_host;
use crate::AppState;
use crate::db::activity;
use crate::error::{AppError, AppResult};
use crate::folder::runner::{self, Remote, RunState, Runner, StepEvent};
use crate::folder::{FolderSpec, StepInfo, check_remote_path};
use crate::settings;
use crate::terminal::TerminalExit;
use crate::terminal::ssh_args::{find_tool, host_options};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderOpened {
    /// Route id: write/resize/close it like a terminal session.
    pub session_id: String,
    pub command: String,
    /// Pass back as `resume` to retry a failed run from the step that failed.
    pub run_id: String,
    pub steps: Vec<StepInfo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderLast {
    pub remote: Option<String>,
    pub local: Option<String>,
}

fn remote_pref(host_id: &str) -> String {
    format!("folder-remote:{host_id}")
}
const LOCAL_PREF: &str = "folder-local";

/// The remote folder last used for this host and the last local folder.
#[tauri::command]
pub fn folder_last(state: State<'_, AppState>, host_id: String) -> AppResult<FolderLast> {
    state.db.tx(|tx| {
        Ok(FolderLast {
            remote: settings::pref(tx, &remote_pref(&host_id))?,
            local: settings::pref(tx, LOCAL_PREF)?,
        })
    })
}

/// Starts (or, with `resume`, continues) a folder download in a terminal tab.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // the IPC argument list
pub fn folder_open(
    app: AppHandle,
    state: State<'_, AppState>,
    spec: FolderSpec,
    resume: Option<String>,
    cols: u16,
    rows: u16,
    on_data: Channel,
    on_exit: Channel<TerminalExit>,
    on_step: Channel<StepEvent>,
) -> AppResult<FolderOpened> {
    // A retry continues the stored run (its archive and progress); otherwise a new one.
    let previous = match &resume {
        Some(id) => state
            .folder_runs
            .lock()
            .map_err(|_| AppError::Other("lock poisoned".into()))?
            .remove(id),
        None => None,
    };
    let run = match previous {
        Some(run) => run,
        None => {
            check_remote_path(&spec.remote_path)?;
            let local = Path::new(spec.local_dir.trim());
            if !local.is_absolute() || !local.is_dir() {
                return Err(AppError::Validation(format!(
                    "choose an existing local folder (not {})",
                    spec.local_dir
                )));
            }
            let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
            RunState::new(id, spec)
        }
    };
    let spec = run.spec.clone();
    let host = live_host(&state, &spec.host_id)?;
    let (options, destination) = host_options(&host, spec.user.as_deref())?;
    let remote = Remote {
        ssh: find_tool("ssh")?,
        scp: find_tool("scp")?,
        options,
        destination,
    };
    state.db.tx(|tx| {
        settings::set_pref(tx, &remote_pref(&host.id), spec.remote_path.trim())?;
        settings::set_pref(tx, LOCAL_PREF, spec.local_dir.trim())
    })?;

    let command = format!(
        "zip {} on {} → {}",
        spec.remote_path.trim(),
        remote.destination,
        spec.local_dir.trim()
    );
    let activity_id = state.db.tx(|tx| {
        activity::start(
            tx,
            &host.id,
            "folder_down",
            serde_json::json!({
                "command": command,
                "remote": spec.remote_path.trim(),
                "local": spec.local_dir.trim(),
                "retry": resume.is_some(),
            }),
        )
    })?;

    let route = uuid::Uuid::new_v4().to_string();
    state.terminals.open_route(&route, cols, rows)?;
    let steps = run
        .steps
        .iter()
        .map(|&kind| StepInfo {
            kind,
            label: kind.label(),
        })
        .collect();
    let run_id = run.id.clone();
    let runner = Runner {
        manager: state.terminals.clone(),
        route: route.clone(),
        remote,
        on_data,
        on_step,
    };
    let finished = move |run: RunState, code: Option<u32>| {
        let state = app.state::<AppState>();
        if let Err(e) = state
            .db
            .tx(|tx| activity::finish(tx, activity_id, code.map(i64::from)))
        {
            tracing::warn!("could not record the transfer's end: {e}");
        }
        // Kept for a retry until it completes.
        if !run.finished
            && let Ok(mut runs) = state.folder_runs.lock()
        {
            runs.insert(run.id.clone(), run);
        }
    };
    runner::spawn(runner, run, on_exit, finished)?;
    tracing::info!(host = %host.alias, "folder transfer started");
    Ok(FolderOpened {
        session_id: route,
        command,
        run_id,
        steps,
    })
}
