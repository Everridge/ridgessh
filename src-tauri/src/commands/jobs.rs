//! File transfers and script runs, each in its own terminal tab.

use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use super::terminal::{Job, TerminalOpened, live_host, spawn_job};
use crate::AppState;
use crate::backup::Reason;
use crate::error::{AppError, AppResult};
use crate::history::Source;
use crate::scp::{self, Direction, TransferSpec};
use crate::scripts::{self, RunMode, Script, ScriptInput};
use crate::settings;
use crate::terminal::TerminalExit;
use crate::terminal::ssh_args::{find_tool, host_options};

fn remote_pref(host_id: &str) -> String {
    format!("scp-remote:{host_id}")
}

#[tauri::command]
pub fn transfer_open(
    app: AppHandle,
    state: State<'_, AppState>,
    spec: TransferSpec,
    cols: u16,
    rows: u16,
    on_data: Channel,
    on_exit: Channel<TerminalExit>,
) -> AppResult<TerminalOpened> {
    let host = live_host(&state, &spec.host_id)?;
    let scp_exe = find_tool("scp")?;
    let args = scp::scp_args(&host, &spec)?;
    if spec.direction == Direction::Upload {
        state
            .db
            .tx(|tx| settings::set_pref(tx, &remote_pref(&host.id), spec.remote.trim()))?;
    }
    let job = Job {
        host: &host,
        program: &scp_exe,
        args,
        kind: match spec.direction {
            Direction::Upload => "scp_up",
            Direction::Download => "scp_down",
        },
        details: serde_json::json!({
            "command": scp::summary(&spec),
            "local": spec.local,
            "remote": spec.remote.trim(),
        }),
        command: format!("scp {} ({})", scp::summary(&spec), host.alias),
    };
    spawn_job(app, &state, job, (cols, rows), on_data, on_exit)
}

/// The remote folder last used for uploads to this host.
#[tauri::command]
pub fn transfer_last_remote(
    state: State<'_, AppState>,
    host_id: String,
) -> AppResult<Option<String>> {
    state.db.tx(|tx| settings::pref(tx, &remote_pref(&host_id)))
}

#[tauri::command]
pub fn scripts_list(state: State<'_, AppState>) -> AppResult<Vec<Script>> {
    state.db.tx(scripts::list)
}

#[tauri::command]
pub async fn script_create(state: State<'_, AppState>, input: ScriptInput) -> AppResult<Script> {
    state.auto_backup(Reason::Periodic)?;
    state.db.tx(|tx| scripts::create(tx, input, Source::User))
}

#[tauri::command]
pub async fn script_update(
    state: State<'_, AppState>,
    id: String,
    input: ScriptInput,
) -> AppResult<Script> {
    state.auto_backup(Reason::Periodic)?;
    state
        .db
        .tx(|tx| scripts::update(tx, &id, input, Source::User))
}

#[tauri::command]
pub async fn script_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.auto_backup(Reason::Periodic)?;
    state.db.tx(|tx| scripts::delete(tx, &id, Source::User))
}

/// Reads a local script file chosen by the user (UTF-8 text, size-limited).
#[tauri::command]
pub fn script_read_file(path: String) -> AppResult<String> {
    let meta = std::fs::metadata(&path)?;
    if meta.len() as usize > scripts::MAX_SCRIPT_BYTES {
        return Err(AppError::Validation(format!(
            "scripts can be at most {} KB",
            scripts::MAX_SCRIPT_BYTES / 1000
        )));
    }
    String::from_utf8(std::fs::read(&path)?)
        .map_err(|_| AppError::Validation(format!("{path} is not a UTF-8 text file")))
}

/// What to run: a library script, or ad-hoc content (e.g. a local .sh file).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptRun {
    pub host_id: String,
    pub script_id: Option<String>,
    /// Ad-hoc runs: display name and content.
    pub name: Option<String>,
    pub content: Option<String>,
    pub interpreter: Option<String>,
    pub mode: Option<RunMode>,
    #[serde(default)]
    pub args: String,
    /// Connect as this user (`user@alias`, see `target_args`).
    #[serde(default)]
    pub user: Option<String>,
}

/// Windows refuses longer command lines (32 767 characters).
const MAX_COMMAND_LINE: usize = 30_000;

#[tauri::command]
pub fn script_open(
    app: AppHandle,
    state: State<'_, AppState>,
    run: ScriptRun,
    cols: u16,
    rows: u16,
    on_data: Channel,
    on_exit: Channel<TerminalExit>,
) -> AppResult<TerminalOpened> {
    let host = live_host(&state, &run.host_id)?;
    let (name, content, interpreter, mode) = match &run.script_id {
        Some(id) => {
            let s = state.db.tx(|tx| scripts::get(tx, id))?;
            (
                s.name,
                s.content,
                run.interpreter.clone().unwrap_or(s.interpreter),
                run.mode.unwrap_or(s.run_mode),
            )
        }
        None => (
            run.name.clone().unwrap_or_else(|| "script".into()),
            run.content
                .clone()
                .ok_or_else(|| AppError::Validation("nothing to run".into()))?,
            run.interpreter.clone().unwrap_or_else(|| "bash".into()),
            run.mode.unwrap_or_default(),
        ),
    };
    let remote = scripts::remote_command(&content, &interpreter, mode, &run.args)?;
    let ssh = find_tool("ssh")?;
    let (mut args, destination) = host_options(&host, run.user.as_deref())?;
    // A remote terminal, so the script (or sudo) can ask for input.
    args.extend(["-t".into(), "--".into(), destination, remote]);
    if args.iter().map(|a| a.len() + 3).sum::<usize>() > MAX_COMMAND_LINE {
        return Err(AppError::Validation(format!(
            "{name} is too large to send in one command (keep scripts under {} KB)",
            scripts::MAX_SCRIPT_BYTES / 1000
        )));
    }
    let mode_label = match mode {
        RunMode::Stream => "stream",
        RunMode::Copy => "copy & run",
    };
    let job = Job {
        host: &host,
        program: &ssh,
        args,
        kind: "script",
        details: serde_json::json!({
            "command": format!("{name} ({mode_label})"),
            "script": name,
            "scriptId": run.script_id,
            "mode": mode,
            "args": run.args,
        }),
        command: format!(
            "ssh {} — run {name} ({mode_label}) {}",
            host.alias, run.args
        )
        .trim_end()
        .into(),
    };
    spawn_job(app, &state, job, (cols, rows), on_data, on_exit)
}
