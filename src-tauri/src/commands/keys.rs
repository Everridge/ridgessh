//! New hosts with a generated key, and installing a host's public key with a password login.

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use super::terminal::{Job, TerminalOpened, live_host, spawn_job};
use crate::AppState;
use crate::backup::Reason;
use crate::db::hosts::{Host, HostInput};
use crate::error::{AppError, AppResult};
use crate::ssh_keys;
use crate::terminal::TerminalExit;
use crate::terminal::ssh_args::{find_tool, host_options};

fn home() -> AppResult<std::path::PathBuf> {
    dirs::home_dir().ok_or_else(|| AppError::NotFound("the home folder".into()))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostWithKey {
    pub host: Host,
    pub public_key: String,
}

/// Creates a host and generates an ed25519 key pair at its identity file (a new file in
/// `~/.ssh`). Neither is kept if the other fails.
#[tauri::command]
pub async fn host_create_with_key(
    state: State<'_, AppState>,
    input: HostInput,
) -> AppResult<HostWithKey> {
    state.auto_backup(Reason::Periodic)?;
    let home = home()?;
    let (host, public_key) = state
        .db
        .tx(|tx| ssh_keys::create_host_with_key(tx, input, &home))?;
    tracing::info!(host = %host.alias, "host created with a new key");
    Ok(HostWithKey { host, public_key })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInstallSpec {
    pub host_id: String,
    /// Remove the server's entries from known_hosts first (reinstalled server, reused IP).
    #[serde(default)]
    pub forget_host_key: bool,
}

/// Logs in with a password (typed at ssh's prompt in the tab) and adds the host's public key
/// (`<IdentityFile>.pub`) to the server's `~/.ssh/authorized_keys`.
#[tauri::command]
pub fn key_install_open(
    app: AppHandle,
    state: State<'_, AppState>,
    spec: KeyInstallSpec,
    cols: u16,
    rows: u16,
    on_data: Channel,
    on_exit: Channel<TerminalExit>,
) -> AppResult<TerminalOpened> {
    let host = live_host(&state, &spec.host_id)?;
    let identity_file = host.identity_file.clone().ok_or_else(|| {
        AppError::Validation(format!("{} has no identity file to install", host.alias))
    })?;
    let home = home()?;
    let public_key = ssh_keys::read_public_key(&identity_file, &home)?;
    let ssh = find_tool("ssh")?;
    let (mut args, destination) = host_options(&host, None)?;

    let mut note = String::new();
    if spec.forget_host_key {
        let removed = ssh_keys::forget_host_key(&ssh, &args, &destination, &home)?;
        note = if removed.is_empty() {
            " (no saved host key to remove)".into()
        } else {
            format!(" (removed saved host key for {})", removed.join(", "))
        };
    }

    // Command-line options come first, so these win over the config file.
    for opt in [
        "PubkeyAuthentication=no",
        "PreferredAuthentications=keyboard-interactive,password",
    ] {
        args.extend(["-o".into(), opt.into()]);
    }
    args.extend([
        "--".into(),
        destination,
        ssh_keys::install_command(&public_key)?,
    ]);
    let job = Job {
        host: &host,
        program: &ssh,
        args,
        kind: "key_install",
        details: serde_json::json!({
            "command": format!("install {identity_file}.pub"),
            "identityFile": identity_file,
            "forgetHostKey": spec.forget_host_key,
        }),
        command: format!(
            "ssh {} — install {identity_file}.pub with a password login{note}",
            host.alias
        ),
    };
    spawn_job(app, &state, job, (cols, rows), on_data, on_exit)
}
