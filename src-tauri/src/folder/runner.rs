//! Runs a folder transfer's steps one after another on a background thread. Every process
//! goes into the same tab through a terminal route (see `TerminalManager::open_route`), so
//! the user can answer prompts; step progress goes to a separate channel.

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};

use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};

use super::{
    EXIT_CHECKSUM, EXIT_EXISTS, EXIT_NO_ACCESS, EXIT_NO_PACKER, EXIT_NO_SPACE, EXIT_NOT_FOUND,
    EXIT_SKIPPED_FILES, EXIT_UNSAFE, Endpoint, Existing, FolderSpec, Manifest, StepKind,
    backup_name, check_command, check_dest_command, cleanup_command, exclude_patterns, find_tar,
    free_name, free_space, local, owner_command, pack_command, parse_manifest, remote_archive,
    remote_inbox, sha256_file, source_name, unpack_remote_command, with_sudo,
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
    /// The archive's checksum, size and top-level folder name, once it exists.
    pub manifest: Option<Manifest>,
    /// The archive on this PC (downloaded, or zipped here for an upload).
    pub local_zip: Option<PathBuf>,
    /// `local_zip` is a temporary file, deleted by the clean-up step.
    pub local_zip_temp: bool,
    /// The scripts' remote command lines, resolved when the run starts (library scripts are
    /// read from the database then).
    pub before_cmd: Option<String>,
    pub after_cmd: Option<String>,
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
            local_zip_temp: false,
            before_cmd: None,
            after_cmd: None,
            finished: false,
        }
    }
}

/// How to reach one server: the host's ssh options and destination (`alias`/`user@alias`).
pub struct Conn {
    pub options: Vec<String>,
    pub destination: String,
}

pub struct Runner {
    pub manager: Arc<TerminalManager>,
    /// The tab's route id.
    pub route: String,
    pub ssh: PathBuf,
    pub scp: PathBuf,
    /// Set when the source is a server.
    pub source: Option<Conn>,
    /// Set when the destination is a server.
    pub dest: Option<Conn>,
    pub on_data: Channel,
    pub on_step: Channel<StepEvent>,
}

enum Outcome {
    Ok,
    Warning(String),
}

type StepResult = AppResult<Option<Outcome>>;

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[90m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

/// Explains a remote script's exit code (see the `EXIT_*` constants).
fn failed(code: u32, what: &str) -> AppError {
    AppError::Other(match code {
        EXIT_NOT_FOUND => "the folder was not found on the server".into(),
        EXIT_NO_PACKER => "the server has no zip/unzip and no python3 (see above)".into(),
        EXIT_NO_SPACE => "not enough free space on the server (see above)".into(),
        EXIT_NO_ACCESS => "no permission for that folder on the server (see above)".into(),
        EXIT_CHECKSUM => "the uploaded archive was damaged in transit".into(),
        EXIT_EXISTS => {
            "the destination already has a folder of that name (you chose to stop)".into()
        }
        EXIT_UNSAFE => "the archive contains unsafe paths; nothing was unpacked".into(),
        255 => "ssh could not connect (see above)".into(),
        n => format!("{what} failed with exit code {n} (see above)"),
    })
}

/// `Ok(None)` for a cancelled process, `Ok(Some(Ok))` for exit 0, otherwise an error.
fn plain(code: Option<u32>, what: &str) -> StepResult {
    match code {
        None => Ok(None),
        Some(0) => Ok(Some(Outcome::Ok)),
        Some(n) => Err(failed(n, what)),
    }
}

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

    fn conn<'a>(&self, c: &'a Option<Conn>, side: &str) -> AppResult<&'a Conn> {
        c.as_ref()
            .ok_or_else(|| AppError::Other(format!("no {side} server for this step")))
    }

    fn ssh(&self, conn: &Conn, remote_command: String) -> AppResult<Option<u32>> {
        self.ssh_with(conn, remote_command, false, false)
    }

    /// `sudo`: run as root; `tty`: give the remote side a terminal so it can prompt (sudo
    /// always needs one).
    fn ssh_with(
        &self,
        conn: &Conn,
        remote_command: String,
        sudo: bool,
        tty: bool,
    ) -> AppResult<Option<u32>> {
        let mut args = conn.options.clone();
        if sudo || tty {
            args.push("-t".into());
        }
        args.push(conn.destination.clone());
        args.push(if sudo {
            with_sudo(&remote_command)
        } else {
            remote_command
        });
        self.run(&self.ssh, args)
    }

    fn scp_down(&self, conn: &Conn, quiet: bool, from: &str, to: &Path) -> AppResult<Option<u32>> {
        let mut args = conn.options.clone();
        if quiet {
            args.push("-q".into());
        }
        args.push(format!("{}:{from}", conn.destination));
        args.push(to.display().to_string());
        self.run(&self.scp, args)
    }

    fn scp_up(&self, conn: &Conn, from: &Path, to: &str) -> AppResult<Option<u32>> {
        let mut args = conn.options.clone();
        args.push(from.display().to_string());
        args.push(format!("{}:{to}", conn.destination));
        self.run(&self.scp, args)
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
        let name = state
            .manifest
            .as_ref()
            .map(|m| m.name.as_str())
            .unwrap_or("");
        let place = match (&state.spec.destination, &self.dest) {
            (Endpoint::Remote { path, .. }, Some(c)) => {
                format!(
                    "Copied to {}:{}/{name}",
                    c.destination,
                    path.trim_end_matches('/')
                )
            }
            _ => format!(
                "Saved in {}",
                state.spec.destination.path().trim_end_matches(['/', '\\'])
            ),
        };
        self.say(&format!("\n{GREEN}✓ Done.{RESET} {place}"));
        Some(0)
    }

    fn run_step(&self, kind: StepKind, state: &mut RunState) -> StepResult {
        let spec = state.spec.clone();
        match kind {
            StepKind::Check => match &spec.source {
                Endpoint::Remote { path, .. } => {
                    let src = self.conn(&self.source, "source")?;
                    let cmd = check_command(path)?;
                    plain(
                        self.ssh_with(src, cmd, spec.sudo_source, false)?,
                        "the check",
                    )
                }
                Endpoint::Local { path } => self.check_local(Path::new(path.trim())),
            },
            StepKind::CheckDest => {
                let dest = self.conn(&self.dest, "destination")?;
                let need_kb = match &spec.source {
                    Endpoint::Local { path } => {
                        Some(local::folder_size(Path::new(path.trim())).1 / 1024)
                    }
                    Endpoint::Remote { .. } => None,
                };
                let cmd = check_dest_command(
                    spec.destination.path(),
                    source_name(&spec).as_deref(),
                    spec.existing,
                    need_kb,
                )?;
                plain(
                    self.ssh_with(dest, cmd, spec.sudo_dest, false)?,
                    "the destination check",
                )
            }
            StepKind::Pack => {
                let src = self.conn(&self.source, "source")?;
                let excludes = exclude_patterns(source_name(&spec).as_deref(), &spec.excludes);
                let cmd = pack_command(spec.source.path(), &remote_archive(&state.id), &excludes)?;
                match self.ssh_with(src, cmd, spec.sudo_source, false)? {
                    Some(EXIT_SKIPPED_FILES) => Ok(Some(Outcome::Warning(
                        "Some files could not be read on the server and are not in the archive."
                            .into(),
                    ))),
                    code => plain(code, "zip"),
                }
            }
            StepKind::PackLocal => self.pack_local(state),
            StepKind::Download => self.download(state),
            StepKind::Unpack => self.unpack_local(state),
            StepKind::Upload => {
                let dest = self.conn(&self.dest, "destination")?;
                let zip = state
                    .local_zip
                    .clone()
                    .ok_or_else(|| AppError::Other("there is no archive to upload".into()))?;
                match self.scp_up(dest, &zip, &remote_inbox(&state.id))? {
                    None => Ok(None),
                    Some(0) => Ok(Some(Outcome::Ok)),
                    Some(n) => Err(AppError::Other(format!("scp failed with exit code {n}"))),
                }
            }
            StepKind::UnpackRemote => {
                let dest = self.conn(&self.dest, "destination")?;
                let m = state
                    .manifest
                    .clone()
                    .ok_or_else(|| AppError::Other("the archive is not ready".into()))?;
                let cmd = unpack_remote_command(
                    spec.destination.path(),
                    &remote_inbox(&state.id),
                    &m.name,
                    spec.existing,
                    m.sha256.as_deref(),
                )?;
                plain(self.ssh_with(dest, cmd, spec.sudo_dest, false)?, "unzip")
            }
            StepKind::SetOwner => {
                let dest = self.conn(&self.dest, "destination")?;
                let (Some(owner), Some(m)) = (&spec.owner, &state.manifest) else {
                    return Ok(Some(Outcome::Ok));
                };
                let cmd = owner_command(spec.destination.path(), &m.name, owner)?;
                plain(self.ssh_with(dest, cmd, spec.sudo_dest, false)?, "chown")
            }
            StepKind::Before => {
                let src = self.conn(&self.source, "source")?;
                let cmd = state
                    .before_cmd
                    .clone()
                    .ok_or_else(|| AppError::Other("the script before is missing".into()))?;
                plain(self.ssh_with(src, cmd, false, true)?, "the script before")
            }
            StepKind::After => {
                let dest = self.conn(&self.dest, "destination")?;
                let cmd = state
                    .after_cmd
                    .clone()
                    .ok_or_else(|| AppError::Other("the script after is missing".into()))?;
                plain(self.ssh_with(dest, cmd, false, true)?, "the script after")
            }
            StepKind::Cleanup => self.cleanup(state),
        }
    }

    fn check_local(&self, dir: &Path) -> StepResult {
        if !dir.is_dir() {
            return Err(AppError::NotFound(format!("folder {}", dir.display())));
        }
        let (files, bytes) = local::folder_size(dir);
        self.say(&format!(
            "Folder:  {}\nSize:    {} MB in {files} files",
            dir.display(),
            bytes / 1_048_576
        ));
        let tmp = std::env::temp_dir();
        if let Some(free) = free_space(&tmp)
            && free < bytes
        {
            return Err(AppError::Other(format!(
                "not enough free space in {} for the archive ({} MB needed at most, {} MB free)",
                tmp.display(),
                bytes / 1_048_576,
                free / 1_048_576
            )));
        }
        Ok(Some(Outcome::Ok))
    }

    fn pack_local(&self, state: &mut RunState) -> StepResult {
        let dir = PathBuf::from(state.spec.source.path());
        let out = std::env::temp_dir().join(format!("ridgessh-{}.zip", state.id));
        self.say(&format!("{DIM}Zipping {}…{RESET}", dir.display()));
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
        let excludes = exclude_patterns(name.as_deref(), &state.spec.excludes);
        let report = match local::pack(&dir, &out, &excludes) {
            Ok(r) => r,
            Err(e) => {
                let _ = std::fs::remove_file(&out);
                return Err(e);
            }
        };
        let bytes = std::fs::metadata(&out)?.len();
        let sha = sha256_file(&out)?;
        self.say(&format!(
            "Zipped {} files ({} MB) into {} MB",
            report.files,
            report.bytes / 1_048_576,
            bytes / 1_048_576
        ));
        if report.excluded > 0 {
            self.say(&format!("Left out {} excluded entries", report.excluded));
        }
        for s in report.skipped.iter().take(20) {
            self.say(&format!("{YELLOW}skipped {s}{RESET}"));
        }
        state.manifest = Some(Manifest {
            sha256: Some(sha),
            bytes,
            name: dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        });
        state.local_zip = Some(out);
        state.local_zip_temp = true;
        Ok(Some(if report.skipped.is_empty() {
            Outcome::Ok
        } else {
            Outcome::Warning(format!(
                "{} entries could not be read and are not in the archive.",
                report.skipped.len()
            ))
        }))
    }

    fn download(&self, state: &mut RunState) -> StepResult {
        let src = self.conn(&self.source, "source")?;
        let archive = remote_archive(&state.id);
        // Into the destination folder, the folder for a kept copy, or a temporary file that
        // the clean-up step deletes after the upload.
        let (dir, temp) = match (&state.spec.destination, &state.spec.keep_copy) {
            (Endpoint::Local { path }, _) => (PathBuf::from(path.trim()), false),
            (Endpoint::Remote { .. }, Some(keep)) => (PathBuf::from(keep.trim()), false),
            (Endpoint::Remote { .. }, None) => (std::env::temp_dir(), true),
        };
        if !dir.is_dir() {
            return Err(AppError::NotFound(format!(
                "local folder {}",
                dir.display()
            )));
        }
        // The summary first: checksum, size and the folder's name.
        let sum_file = dir.join(format!(".ridgessh-{}.sum", state.id));
        let code = self.scp_down(src, true, &format!("{archive}.sum"), &sum_file)?;
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
        match self.scp_down(src, false, &archive, &part)? {
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
        let zip = if temp {
            dir.join(format!("ridgessh-{}.zip", state.id))
        } else {
            free_name(&dir, &manifest.name, ".zip")
        };
        std::fs::rename(&part, &zip)?;
        if !temp {
            self.say(&format!("Saved {}", zip.display()));
        }
        state.local_zip = Some(zip);
        state.local_zip_temp = temp;
        state.manifest = Some(manifest);
        Ok(Some(match warning {
            Some(w) => Outcome::Warning(w),
            None => Outcome::Ok,
        }))
    }

    fn unpack_local(&self, state: &mut RunState) -> StepResult {
        let (Some(zip), Some(manifest)) = (&state.local_zip, &state.manifest) else {
            return Err(AppError::Other("download the archive first".into()));
        };
        let dir = PathBuf::from(state.spec.destination.path());
        let target = dir.join(&manifest.name);
        if target.exists() {
            match state.spec.existing {
                Existing::Stop => {
                    return Err(AppError::Other(format!(
                        "{} already exists (you chose to stop); the archive is kept at {}",
                        target.display(),
                        zip.display()
                    )));
                }
                Existing::Backup => {
                    let backup = backup_name(&dir, &manifest.name);
                    std::fs::rename(&target, &backup)?;
                    self.say(&format!(
                        "{YELLOW}{} existed; moved it to {}{RESET}",
                        target.display(),
                        backup.display()
                    ));
                }
                Existing::Merge => self.say(&format!(
                    "{YELLOW}Unpacking over the existing {}{RESET}",
                    target.display()
                )),
            }
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

    fn cleanup(&self, state: &mut RunState) -> StepResult {
        // Leftovers on a server are harmless (private, in /tmp), so a failed removal is a
        // warning, not a failed transfer; ssh already printed why.
        let mut left = Vec::new();
        let sides = [
            (&self.source, remote_archive(&state.id)),
            (&self.dest, remote_inbox(&state.id)),
        ];
        for (conn, archive) in sides {
            let Some(conn) = conn else { continue };
            match self.ssh(conn, cleanup_command(&archive)?)? {
                None => return Ok(None),
                Some(0) => {}
                Some(_) => left.push(format!("{}:{archive}", conn.destination)),
            }
        }
        if state.local_zip_temp
            && let Some(zip) = &state.local_zip
        {
            match std::fs::remove_file(zip) {
                Ok(()) => self.say("Removed the temporary archive from this PC."),
                Err(_) => left.push(zip.display().to_string()),
            }
        }
        Ok(Some(if left.is_empty() {
            Outcome::Ok
        } else {
            Outcome::Warning(format!("Could not remove {}", left.join(", ")))
        }))
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
