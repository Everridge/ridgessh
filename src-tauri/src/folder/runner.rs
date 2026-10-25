//! Runs a folder transfer's steps one after another on a background thread. Every process
//! goes into the same tab through a terminal route (see `TerminalManager::open_route`), so
//! the user can answer prompts; step progress goes to a separate channel.

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};

use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};

use super::{
    EXIT_NO_ACCESS, EXIT_NO_PACKER, EXIT_NO_SPACE, EXIT_NOT_FOUND, EXIT_SKIPPED_FILES, FolderSpec,
    Manifest, StepKind, backup_name, check_command, cleanup_command, find_tar, free_name,
    free_space, pack_command, parse_manifest, remote_archive, sha256_file,
};
use crate::error::{AppError, AppResult};
use crate::terminal::{SpawnRequest, TerminalExit, TerminalManager};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepStatus {
    Running,
    Done,
    /// Finished with a warning (e.g. some files could not be read).
    Warning,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepEvent {
    pub index: usize,
    pub status: StepStatus,
    pub detail: Option<String>,
}

/// What a run has achieved so far, kept so "Retry" continues where it failed.
#[derive(Debug, Clone)]
pub struct RunState {
    pub id: String,
    pub spec: FolderSpec,
    pub steps: Vec<StepKind>,
    pub done: Vec<StepKind>,
    pub manifest: Option<Manifest>,
    /// The downloaded archive on this PC.
    pub local_zip: Option<PathBuf>,
    pub finished: bool,
}

impl RunState {
    pub fn new(id: String, spec: FolderSpec) -> Self {
        Self {
            steps: super::steps(&spec),
            id,
            spec,
            done: Vec::new(),
            manifest: None,
            local_zip: None,
            finished: false,
        }
    }
}

/// How to reach the server: `ssh`/`scp` plus the host's options and destination.
pub struct Remote {
    pub ssh: PathBuf,
    pub scp: PathBuf,
    pub options: Vec<String>,
    pub destination: String,
}

pub struct Runner {
    pub manager: Arc<TerminalManager>,
    /// The tab's route id.
    pub route: String,
    pub remote: Remote,
    pub on_data: Channel,
    pub on_step: Channel<StepEvent>,
}

enum Outcome {
    Ok,
    Warning(String),
}

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[90m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

impl Runner {
    fn say(&self, text: &str) {
        let text = text.replace('\n', "\r\n");
        let _ = self
            .on_data
            .send(InvokeResponseBody::Raw(format!("{text}\r\n").into_bytes()));
    }

    fn step(&self, index: usize, status: StepStatus, detail: Option<String>) {
        let _ = self.on_step.send(StepEvent {
            index,
            status,
            detail,
        });
    }

    fn cancelled(&self) -> bool {
        self.manager.route_state(&self.route).1
    }

    /// Runs one process in the tab and waits for it. `None` = killed (tab closed).
    fn run(&self, program: &Path, args: Vec<String>) -> AppResult<Option<u32>> {
        let ((cols, rows), cancelled) = self.manager.route_state(&self.route);
        if cancelled {
            return Ok(None);
        }
        let (tx, rx) = mpsc::channel();
        let data = self.on_data.clone();
        let session = self.manager.spawn_with(
            SpawnRequest {
                program,
                args,
                cols,
                rows,
            },
            Box::new(move |b| data.send(InvokeResponseBody::Raw(b.to_vec())).is_ok()),
            move |code| {
                let _ = tx.send(code);
            },
        )?;
        self.manager.set_route(&self.route, Some(&session))?;
        let code = rx.recv().unwrap_or(None);
        self.manager.set_route(&self.route, None)?;
        // A kill reports an exit code too; what matters is whether the tab was closed.
        Ok(if self.cancelled() { None } else { code })
    }

    fn ssh(&self, remote_command: String) -> AppResult<Option<u32>> {
        let mut args = self.remote.options.clone();
        args.push(self.remote.destination.clone());
        args.push(remote_command);
        self.run(&self.remote.ssh, args)
    }

    fn scp(&self, quiet: bool, from: &str, to: &Path) -> AppResult<Option<u32>> {
        let mut args = self.remote.options.clone();
        if quiet {
            args.push("-q".into());
        }
        args.push(format!("{}:{from}", self.remote.destination));
        args.push(to.display().to_string());
        self.run(&self.remote.scp, args)
    }

    /// Runs the steps that are not done yet. Returns the tab's exit code: `Some(0)` all done,
    /// `Some(n)` failed, `None` cancelled.
    pub fn execute(&self, state: &mut RunState) -> Option<u32> {
        let total = state.steps.len();
        for (i, kind) in state.steps.clone().into_iter().enumerate() {
            if state.done.contains(&kind) {
                self.step(
                    i,
                    StepStatus::Skipped,
                    Some("done in an earlier attempt".into()),
                );
                continue;
            }
            self.say(&format!(
                "\n{BOLD}▶ Step {}/{total}: {}{RESET}",
                i + 1,
                kind.label()
            ));
            self.step(i, StepStatus::Running, None);
            match self.run_step(kind, state) {
                Ok(None) => {
                    self.say(&format!("{YELLOW}Cancelled.{RESET}"));
                    return None;
                }
                Ok(Some(Outcome::Ok)) => {
                    state.done.push(kind);
                    self.step(i, StepStatus::Done, None);
                }
                Ok(Some(Outcome::Warning(w))) => {
                    state.done.push(kind);
                    self.say(&format!("{YELLOW}{w}{RESET}"));
                    self.step(i, StepStatus::Warning, Some(w));
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.say(&format!(
                        "{RED}✗ {}: {msg}{RESET}\n{DIM}Fix the problem, then use Retry to continue from this step.{RESET}",
                        kind.label()
                    ));
                    self.step(i, StepStatus::Failed, Some(msg));
                    return Some(1);
                }
            }
        }
        state.finished = true;
        let place = state
            .local_zip
            .as_ref()
            .and_then(|z| z.parent())
            .map(|d| d.display().to_string())
            .unwrap_or_default();
        self.say(&format!("\n{GREEN}✓ Done.{RESET} Saved in {place}"));
        Some(0)
    }

    fn run_step(&self, kind: StepKind, state: &mut RunState) -> AppResult<Option<Outcome>> {
        let archive = remote_archive(&state.id);
        let spec = &state.spec;
        let failed = |code: u32, what: &str| -> AppError {
            AppError::Other(match code {
                EXIT_NOT_FOUND => "the folder was not found on the server".into(),
                EXIT_NO_PACKER => "neither zip nor python3 is installed on the server".into(),
                EXIT_NO_SPACE => "not enough free space in /tmp on the server".into(),
                EXIT_NO_ACCESS => "no permission to read the folder".into(),
                255 => "ssh could not connect (see above)".into(),
                n => format!("{what} failed with exit code {n} (see above)"),
            })
        };
        match kind {
            StepKind::Check => match self.ssh(check_command(&spec.remote_path)?)? {
                None => Ok(None),
                Some(0) => Ok(Some(Outcome::Ok)),
                Some(n) => Err(failed(n, "the check")),
            },
            StepKind::Pack => match self.ssh(pack_command(&spec.remote_path, &archive)?)? {
                None => Ok(None),
                Some(0) => Ok(Some(Outcome::Ok)),
                Some(EXIT_SKIPPED_FILES) => Ok(Some(Outcome::Warning(
                    "Some files could not be read on the server and are not in the archive.".into(),
                ))),
                Some(n) => Err(failed(n, "zip")),
            },
            StepKind::Download => self.download(state, &archive),
            StepKind::Unpack => self.unpack(state),
            StepKind::Cleanup => match self.ssh(cleanup_command(&archive)?)? {
                None => Ok(None),
                Some(0) => Ok(Some(Outcome::Ok)),
                Some(n) => Err(failed(n, "removing the temporary archive")),
            },
        }
    }

    fn download(&self, state: &mut RunState, archive: &str) -> AppResult<Option<Outcome>> {
        let dir = PathBuf::from(&state.spec.local_dir);
        if !dir.is_dir() {
            return Err(AppError::NotFound(format!(
                "local folder {}",
                dir.display()
            )));
        }
        // The summary first: checksum, size and the folder's name.
        let sum_file = dir.join(format!(".ridgessh-{}.sum", state.id));
        let code = self.scp(true, &format!("{archive}.sum"), &sum_file)?;
        let text = std::fs::read_to_string(&sum_file);
        let _ = std::fs::remove_file(&sum_file);
        match code {
            None => return Ok(None),
            Some(0) => {}
            Some(n) => {
                return Err(AppError::Other(format!(
                    "could not fetch the archive summary (scp exit code {n}); run the whole transfer again"
                )));
            }
        }
        let manifest = parse_manifest(&text?)?;
        if let Some(free) = free_space(&dir)
            && free < manifest.bytes
        {
            return Err(AppError::Other(format!(
                "not enough free space here: the archive is {} MB, {} MB are free",
                manifest.bytes / 1_048_576,
                free / 1_048_576
            )));
        }

        let part = dir.join(format!(".ridgessh-{}.zip.part", state.id));
        let code = self.scp(false, archive, &part)?;
        match code {
            None => {
                let _ = std::fs::remove_file(&part);
                return Ok(None);
            }
            Some(0) => {}
            Some(n) => {
                let _ = std::fs::remove_file(&part);
                return Err(AppError::Other(format!("scp failed with exit code {n}")));
            }
        }
        let got = std::fs::metadata(&part)?.len();
        if got != manifest.bytes {
            let _ = std::fs::remove_file(&part);
            return Err(AppError::Other(format!(
                "the download is incomplete ({got} of {} bytes)",
                manifest.bytes
            )));
        }
        let warning = match &manifest.sha256 {
            Some(expected) => {
                self.say(&format!("{DIM}Verifying the checksum…{RESET}"));
                let actual = sha256_file(&part)?;
                if &actual != expected {
                    let _ = std::fs::remove_file(&part);
                    return Err(AppError::Other(
                        "the checksum does not match: the archive was damaged in transit".into(),
                    ));
                }
                self.say(&format!("{GREEN}Checksum OK{RESET} (SHA-256 {actual})"));
                None
            }
            None => {
                Some("The server has no SHA-256 tool, so only the size was checked.".to_string())
            }
        };
        let zip = free_name(&dir, &manifest.name, ".zip");
        std::fs::rename(&part, &zip)?;
        self.say(&format!("Saved {}", zip.display()));
        state.local_zip = Some(zip);
        state.manifest = Some(manifest);
        Ok(Some(match warning {
            Some(w) => Outcome::Warning(w),
            None => Outcome::Ok,
        }))
    }

    fn unpack(&self, state: &mut RunState) -> AppResult<Option<Outcome>> {
        let (Some(zip), Some(manifest)) = (&state.local_zip, &state.manifest) else {
            return Err(AppError::Other("download the archive first".into()));
        };
        let dir = PathBuf::from(&state.spec.local_dir);
        let target = dir.join(&manifest.name);
        if target.exists() {
            let backup = backup_name(&dir, &manifest.name);
            std::fs::rename(&target, &backup)?;
            self.say(&format!(
                "{YELLOW}{} existed; moved it to {}{RESET}",
                target.display(),
                backup.display()
            ));
        }
        let tar = find_tar()?;
        let args = vec![
            "-xf".to_string(),
            zip.display().to_string(),
            "-C".to_string(),
            dir.display().to_string(),
        ];
        match self.run(&tar, args)? {
            None => Ok(None),
            Some(0) => {
                self.say(&format!("Unpacked into {}", target.display()));
                if !state.spec.keep_zip {
                    std::fs::remove_file(zip)?;
                    self.say(&format!("{DIM}Deleted {}{RESET}", zip.display()));
                }
                Ok(Some(Outcome::Ok))
            }
            Some(n) => Err(AppError::Other(format!(
                "tar could not unpack the archive (exit code {n}); it is kept at {}",
                zip.display()
            ))),
        }
    }
}

/// Runs `state` on `runner` and reports the end on `on_exit`.
pub fn spawn(
    runner: Runner,
    mut state: RunState,
    on_exit: Channel<TerminalExit>,
    finished: impl FnOnce(RunState, Option<u32>) + Send + 'static,
) -> AppResult<()> {
    std::thread::Builder::new()
        .name(format!("folder-{}", state.id))
        .spawn(move || {
            let code = runner.execute(&mut state);
            runner.manager.close_route(&runner.route);
            finished(state, code);
            let _ = on_exit.send(TerminalExit { exit_code: code });
        })?;
    Ok(())
}
