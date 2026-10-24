use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::AppState;
use crate::db::activity::{self, Activity};
use crate::db::hosts;
use crate::error::{AppError, AppResult};
use crate::terminal::ssh_args::{find_tool, target_args};
use crate::terminal::{self, SpawnRequest, TerminalExit};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOpened {
    pub session_id: String,
    /// The command line that was started, for display.
    pub command: String,
}

fn display_command(args: &[String]) -> String {
    std::iter::once("ssh".to_string())
        .chain(args.iter().map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        }))
        .collect::<Vec<_>>()
        .join(" ")
}

fn live_host(state: &AppState, host_id: &str) -> AppResult<hosts::Host> {
    let host = state.db.tx(|tx| hosts::get(tx, host_id))?;
    if host.deleted_at.is_some() {
        return Err(AppError::Conflict(
            "restore the host from trash before connecting".into(),
        ));
    }
    Ok(host)
}

/// Opens an embedded ssh session to `host_id`.
#[tauri::command]
pub fn terminal_open(
    app: AppHandle,
    state: State<'_, AppState>,
    host_id: String,
    cols: u16,
    rows: u16,
    on_data: Channel,
    on_exit: Channel<TerminalExit>,
) -> AppResult<TerminalOpened> {
    let host = live_host(&state, &host_id)?;
    let ssh = find_tool("ssh")?;
    let args = target_args(&host)?;
    let command = display_command(&args);
    let activity_id = state.db.tx(|tx| {
        activity::start(
            tx,
            &host.id,
            "terminal",
            serde_json::json!({ "command": command }),
        )
    })?;

    let finished = move |code: Option<u32>| {
        let state = app.state::<AppState>();
        if let Err(e) = state
            .db
            .tx(|tx| activity::finish(tx, activity_id, code.map(i64::from)))
        {
            tracing::warn!("could not record session end: {e}");
        }
    };
    let session_id = state.terminals.spawn(
        SpawnRequest {
            program: &ssh,
            args,
            cols,
            rows,
        },
        on_data,
        on_exit,
        finished,
    )?;
    tracing::info!(host = %host.alias, "terminal opened");
    Ok(TerminalOpened {
        session_id,
        command,
    })
}

#[tauri::command]
pub fn terminal_write(
    state: State<'_, AppState>,
    session_id: String,
    data: String,
) -> AppResult<()> {
    state.terminals.write(&session_id, data.as_bytes())
}

#[tauri::command]
pub fn terminal_resize(
    state: State<'_, AppState>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> AppResult<()> {
    state.terminals.resize(&session_id, cols, rows)
}

#[tauri::command]
pub fn terminal_close(state: State<'_, AppState>, session_id: String) -> AppResult<()> {
    state.terminals.close(&session_id)
}

/// Opens the host in a separate console window instead of the embedded terminal.
#[tauri::command]
pub fn terminal_open_external(state: State<'_, AppState>, host_id: String) -> AppResult<()> {
    let host = live_host(&state, &host_id)?;
    let ssh = find_tool("ssh")?;
    let args = target_args(&host)?;
    terminal::open_external(&ssh, &args)?;
    let command = display_command(&args);
    state.db.tx(|tx| {
        let id = activity::start(
            tx,
            &host.id,
            "terminal_external",
            serde_json::json!({ "command": command }),
        )?;
        // The external process is not tracked; only the launch is recorded.
        activity::finish(tx, id, None)
    })
}

#[tauri::command]
pub fn host_activity(
    state: State<'_, AppState>,
    host_id: String,
    limit: Option<u32>,
) -> AppResult<Vec<Activity>> {
    state
        .db
        .tx(|tx| activity::for_host(tx, &host_id, limit.unwrap_or(50)))
}
