//! Folder transfers (M11) in a terminal tab.

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use super::terminal::live_host;
use crate::AppState;
use crate::db::activity;
use crate::error::{AppError, AppResult};
use crate::folder::browse::{self, Listing};
use crate::folder::recipes::{self, Recipe};
use crate::folder::runner::{self, Conn, RunState, Runner, StepEvent};
use crate::folder::{Endpoint, FolderSpec, Hook, StepInfo, check_spec};
use crate::scripts;
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

/// Paths used last time, to suggest them again.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderLast {
    /// The folder last copied from this host.
    pub remote: Option<String>,
    /// The folder last copied into on this host.
    pub dest: Option<String>,
    /// The last folder on this PC (either side).
    pub local: Option<String>,
}

fn source_pref(host_id: &str) -> String {
    format!("folder-remote:{host_id}")
}
fn dest_pref(host_id: &str) -> String {
    format!("folder-dest:{host_id}")
}
const LOCAL_PREF: &str = "folder-local";

#[tauri::command]
pub fn folder_last(state: State<'_, AppState>, host_id: String) -> AppResult<FolderLast> {
    state.db.tx(|tx| {
        Ok(FolderLast {
            remote: settings::pref(tx, &source_pref(&host_id))?,
            dest: settings::pref(tx, &dest_pref(&host_id))?,
            local: settings::pref(tx, LOCAL_PREF)?,
        })
    })
}

/// Connection details for a server endpoint (`None` for this PC).
fn conn_for(state: &AppState, end: &Endpoint) -> AppResult<Option<(Conn, String)>> {
    let Endpoint::Remote { host_id, user, .. } = end else {
        return Ok(None);
    };
    let host = live_host(state, host_id)?;
    let (options, destination) = host_options(&host, user.as_deref())?;
    Ok(Some((
        Conn {
            options,
            destination,
        },
        host.id,
    )))
}

fn describe(end: &Endpoint, conn: Option<&Conn>) -> String {
    match (end, conn) {
        (Endpoint::Remote { path, .. }, Some(c)) => format!("{}:{}", c.destination, path.trim()),
        _ => end.path().to_string(),
    }
}

/// The remote command line for a script around the transfer.
fn hook_command(state: &AppState, hook: &Hook) -> AppResult<String> {
    match (&hook.script_id, &hook.command) {
        (Some(id), _) => {
            let script = state.db.tx(|tx| scripts::get(tx, id))?;
            scripts::remote_command(
                &script.content,
                &script.interpreter,
                script.run_mode,
                &script.default_args,
            )
        }
        // Run by the login shell, exactly as typed in a terminal.
        (None, Some(command)) => Ok(command.trim().to_string()),
        (None, None) => Err(AppError::Validation("the script is empty".into())),
    }
}

/// Starts (or, with `resume`, continues) a folder transfer in a terminal tab.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // the IPC argument list
pub fn folder_open(
    app: AppHandle,
    state: State<'_, AppState>,
    spec: FolderSpec,
    resume: Option<String>,
    recipe_id: Option<String>,
    cols: u16,
    rows: u16,
    on_data: Channel,
    on_exit: Channel<TerminalExit>,
    on_step: Channel<StepEvent>,
) -> AppResult<FolderOpened> {
    // A retry continues the stored run (its archives and progress); otherwise a new one.
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
            check_spec(&spec)?;
            let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
            let mut run = RunState::new(id, spec);
            run.before_cmd = run
                .spec
                .before
                .as_ref()
                .map(|h| hook_command(&state, h))
                .transpose()?;
            run.after_cmd = run
                .spec
                .after
                .as_ref()
                .map(|h| hook_command(&state, h))
                .transpose()?;
            run
        }
    };
    let spec = run.spec.clone();
    let source = conn_for(&state, &spec.source)?;
    let dest = conn_for(&state, &spec.destination)?;
    let command = format!(
        "zip {} → {}",
        describe(&spec.source, source.as_ref().map(|s| &s.0)),
        describe(&spec.destination, dest.as_ref().map(|d| &d.0)),
    );

    let kind = match (&source, &dest) {
        (Some(_), None) => "folder_down",
        (None, Some(_)) => "folder_up",
        _ => "folder_copy",
    };
    let details = serde_json::json!({
        "command": command,
        "source": spec.source.path(),
        "destination": spec.destination.path(),
        "retry": resume.is_some(),
    });
    // One activity entry per server involved, so each host's log shows the transfer.
    let activities = state.db.tx(|tx| {
        if let Some((_, host_id)) = &source {
            settings::set_pref(tx, &source_pref(host_id), spec.source.path())?;
        }
        if let Some((_, host_id)) = &dest {
            settings::set_pref(tx, &dest_pref(host_id), spec.destination.path())?;
        }
        for end in [&spec.source, &spec.destination] {
            if let Endpoint::Local { path } = end {
                settings::set_pref(tx, LOCAL_PREF, path.trim())?;
            }
        }
        [&source, &dest]
            .into_iter()
            .flatten()
            .map(|(_, host_id)| activity::start(tx, host_id, kind, details.clone()))
            .collect::<AppResult<Vec<i64>>>()
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
        ssh: find_tool("ssh")?,
        scp: find_tool("scp")?,
        source: source.map(|s| s.0),
        dest: dest.map(|d| d.0),
        on_data,
        on_step,
    };
    if let Some(id) = &recipe_id {
        state.db.tx(|tx| recipes::record_run(tx, id, None))?;
    }
    let finished = move |run: RunState, code: Option<u32>| {
        let state = app.state::<AppState>();
        if let Some(id) = &recipe_id
            && let Err(e) = state
                .db
                .tx(|tx| recipes::record_run(tx, id, code.map(i64::from)))
        {
            tracing::warn!("could not record the recipe's run: {e}");
        }
        for id in activities {
            if let Err(e) = state
                .db
                .tx(|tx| activity::finish(tx, id, code.map(i64::from)))
            {
                tracing::warn!("could not record the transfer's end: {e}");
            }
        }
        // Kept for a retry until it completes.
        if !run.finished
            && let Ok(mut runs) = state.folder_runs.lock()
        {
            runs.insert(run.id.clone(), run);
        }
    };
    runner::spawn(runner, run, on_exit, finished)?;
    tracing::info!(kind, "folder transfer started");
    Ok(FolderOpened {
        session_id: route,
        command,
        run_id,
        steps,
    })
}

#[tauri::command]
pub fn recipes_list(state: State<'_, AppState>) -> AppResult<Vec<Recipe>> {
    state.db.tx(recipes::list)
}

/// Creates a recipe (`id` = None) or updates one.
#[tauri::command]
pub fn recipe_save(
    state: State<'_, AppState>,
    id: Option<String>,
    name: String,
    spec: FolderSpec,
) -> AppResult<Recipe> {
    state
        .db
        .tx(|tx| recipes::save(tx, id.as_deref(), &name, &spec))
}

#[tauri::command]
pub fn recipe_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.db.tx(|tx| recipes::delete(tx, &id))
}

/// Lists a folder on a server (key-based logins only; see `folder::browse`).
#[tauri::command]
pub async fn folder_browse(
    state: State<'_, AppState>,
    host_id: String,
    user: Option<String>,
    path: String,
) -> AppResult<Listing> {
    let host = live_host(&state, &host_id)?;
    let (options, destination) = host_options(&host, user.as_deref())?;
    let ssh = find_tool("ssh")?;
    tauri::async_runtime::spawn_blocking(move || browse::run(&ssh, &options, &destination, &path))
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
}
