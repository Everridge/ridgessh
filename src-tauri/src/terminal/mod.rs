//! PTY-backed ssh sessions streamed to xterm.js (M3).
//!
//! Each session runs the system `ssh` in a pseudo terminal (ConPTY on Windows). Output is
//! pushed to the webview as raw bytes over a Tauri channel; a second channel reports the exit.
//!
//! Lifecycle: a waiter thread blocks on the child. When it exits, the session is removed from
//! the map, which drops the PTY master — on Windows that is what lets the reader thread see EOF
//! — then the reader is joined so all output is delivered before the exit event.

pub mod ssh_args;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};

use crate::error::{AppError, AppResult};

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalExit {
    pub exit_code: Option<u32>,
}

/// A stable id for a tab that runs several processes one after another (folder transfers).
/// Writes, resizes and close requests to it go to whichever process is current; input that
/// arrives between processes is held for the next one (ConPTY waits for the reply to its
/// startup cursor-position query, so it must not be lost).
#[derive(Default)]
struct Route {
    current: Option<String>,
    pending: Vec<u8>,
    cancelled: bool,
    size: (u16, u16),
}

#[derive(Default)]
pub struct TerminalManager {
    sessions: Mutex<HashMap<String, Session>>,
    routes: Mutex<HashMap<String, Route>>,
}

/// Receives a process's output; returning `false` stops reading (the webview is gone).
pub type DataSink = Box<dyn FnMut(&[u8]) -> bool + Send>;

fn lock_err<T>(_: T) -> AppError {
    AppError::Other("terminal lock poisoned".into())
}

fn pty_err(e: impl std::fmt::Display) -> AppError {
    AppError::Other(format!("terminal error: {e}"))
}

pub struct SpawnRequest<'a> {
    pub program: &'a Path,
    pub args: Vec<String>,
    pub cols: u16,
    pub rows: u16,
}

impl TerminalManager {
    /// Starts `program args` in a new PTY. `on_finished` runs on the waiter thread after the
    /// process exited and all output was delivered, just before `on_exit` is notified.
    pub fn spawn(
        self: &Arc<Self>,
        req: SpawnRequest<'_>,
        on_data: Channel,
        on_exit: Channel<TerminalExit>,
        on_finished: impl FnOnce(Option<u32>) + Send + 'static,
    ) -> AppResult<String> {
        self.spawn_with(
            req,
            Box::new(move |bytes| {
                on_data
                    .send(InvokeResponseBody::Raw(bytes.to_vec()))
                    .is_ok()
            }),
            move |code| {
                on_finished(code);
                let _ = on_exit.send(TerminalExit { exit_code: code });
            },
        )
    }

    /// Like [`spawn`](Self::spawn), with output going to `on_data`; `on_finished` runs after
    /// the process exited and all its output was delivered.
    pub fn spawn_with(
        self: &Arc<Self>,
        req: SpawnRequest<'_>,
        mut on_data: DataSink,
        on_finished: impl FnOnce(Option<u32>) + Send + 'static,
    ) -> AppResult<String> {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: req.rows.max(1),
                cols: req.cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(pty_err)?;

        let mut cmd = CommandBuilder::new(req.program);
        cmd.args(&req.args);
        cmd.env("TERM", "xterm-256color");
        for (k, v) in gui_env_fixes() {
            cmd.env(k, v);
        }
        if let Some(home) = dirs::home_dir() {
            cmd.cwd(home);
        }
        let mut child = pty.slave.spawn_command(cmd).map_err(pty_err)?;
        drop(pty.slave);

        let mut reader = pty.master.try_clone_reader().map_err(pty_err)?;
        let writer = pty.master.take_writer().map_err(pty_err)?;
        let killer = child.clone_killer();
        let id = uuid::Uuid::new_v4().to_string();

        self.sessions.lock().map_err(lock_err)?.insert(
            id.clone(),
            Session {
                master: pty.master,
                writer,
                killer,
            },
        );

        let reader_thread = std::thread::Builder::new()
            .name(format!("pty-read-{id}"))
            .spawn(move || {
                let mut buf = [0u8; 16 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if !on_data(&buf[..n]) {
                                break; // webview gone
                            }
                        }
                    }
                }
            })?;

        let session_id = id.clone();
        let this = Arc::clone(self);
        std::thread::Builder::new()
            .name(format!("pty-wait-{id}"))
            .spawn(move || {
                let code = child.wait().ok().map(|s| s.exit_code());
                // Dropping the session closes the PTY so the reader reaches EOF.
                if let Ok(mut sessions) = this.sessions.lock() {
                    sessions.remove(&session_id);
                }
                let _ = reader_thread.join();
                on_finished(code);
            })?;

        Ok(id)
    }

    /// Starts a route (see [`Route`]) with the tab's current size.
    pub fn open_route(&self, id: &str, cols: u16, rows: u16) -> AppResult<()> {
        self.routes.lock().map_err(lock_err)?.insert(
            id.to_string(),
            Route {
                size: (cols, rows),
                ..Default::default()
            },
        );
        Ok(())
    }

    /// Points the route at `session` (or at nothing between processes), handing over input
    /// that was typed in the meantime.
    pub fn set_route(&self, id: &str, session: Option<&str>) -> AppResult<()> {
        // Held while flushing, so held input goes out before anything typed after it.
        // Lock order everywhere: routes, then sessions.
        let mut routes = self.routes.lock().map_err(lock_err)?;
        let Some(route) = routes.get_mut(id) else {
            return Ok(());
        };
        route.current = session.map(str::to_string);
        if let Some(s) = session
            && !route.pending.is_empty()
        {
            let pending = std::mem::take(&mut route.pending);
            self.write_session(s, &pending)?;
        }
        Ok(())
    }

    /// The size for the route's next process, and whether it was closed.
    pub fn route_state(&self, id: &str) -> ((u16, u16), bool) {
        self.routes
            .lock()
            .ok()
            .and_then(|r| r.get(id).map(|r| (r.size, r.cancelled)))
            .unwrap_or(((80, 24), true))
    }

    pub fn close_route(&self, id: &str) {
        if let Ok(mut routes) = self.routes.lock() {
            routes.remove(id);
        }
    }

    /// Where input for `id` goes: a session id, or `None` to hold it (a route between
    /// processes). Plain session ids map to themselves.
    fn resolve(&self, id: &str, data: Option<&[u8]>) -> AppResult<Option<String>> {
        let mut routes = self.routes.lock().map_err(lock_err)?;
        match routes.get_mut(id) {
            None => Ok(Some(id.to_string())),
            Some(route) => {
                if route.current.is_none()
                    && let Some(d) = data
                {
                    route.pending.extend_from_slice(d);
                }
                Ok(route.current.clone())
            }
        }
    }

    pub fn write(&self, id: &str, data: &[u8]) -> AppResult<()> {
        match self.resolve(id, Some(data))? {
            Some(session) => self.write_session(&session, data),
            None => Ok(()),
        }
    }

    fn write_session(&self, id: &str, data: &[u8]) -> AppResult<()> {
        let mut sessions = self.sessions.lock().map_err(lock_err)?;
        let s = sessions
            .get_mut(id)
            .ok_or_else(|| AppError::NotFound(format!("session {id}")))?;
        s.writer.write_all(data)?;
        s.writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> AppResult<()> {
        if let Some(route) = self.routes.lock().map_err(lock_err)?.get_mut(id) {
            route.size = (cols, rows);
        }
        let Some(id) = self.resolve(id, None)? else {
            return Ok(());
        };
        let id = id.as_str();
        let sessions = self.sessions.lock().map_err(lock_err)?;
        let s = sessions
            .get(id)
            .ok_or_else(|| AppError::NotFound(format!("session {id}")))?;
        s.master
            .resize(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(pty_err)
    }

    /// Kills the process (for a route: cancels it and kills its current process); cleanup
    /// happens on the waiter thread.
    pub fn close(&self, id: &str) -> AppResult<()> {
        if let Some(route) = self.routes.lock().map_err(lock_err)?.get_mut(id) {
            route.cancelled = true;
        }
        let Some(id) = self.resolve(id, None)? else {
            return Ok(());
        };
        let mut sessions = self.sessions.lock().map_err(lock_err)?;
        if let Some(s) = sessions.get_mut(&id) {
            kill(&mut *s.killer)?;
        }
        Ok(())
    }

    pub fn kill_all(&self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            for s in sessions.values_mut() {
                let _ = kill(&mut *s.killer);
            }
        }
    }

    pub fn count(&self) -> usize {
        self.sessions.lock().map(|s| s.len()).unwrap_or(0)
    }
}

/// `portable-pty` on Windows reports a successful TerminateProcess as "os error 0".
fn kill(killer: &mut (dyn ChildKiller + Send + Sync)) -> AppResult<()> {
    match killer.kill() {
        Err(e) if e.raw_os_error() != Some(0) => Err(pty_err(e)),
        _ => Ok(()),
    }
}

/// Environment fixes for apps started from Finder or the Dock on macOS, which get launchd's
/// bare environment: no `LANG` (remote shells then mangle UTF-8) and a `PATH` without Homebrew
/// (where `ProxyCommand` helpers usually live).
fn gui_env_fixes() -> Vec<(String, String)> {
    let mut out = Vec::new();
    if cfg!(target_os = "macos") {
        if std::env::var_os("LANG").is_none() && std::env::var_os("LC_ALL").is_none() {
            out.push(("LANG".into(), "en_US.UTF-8".into()));
        }
        let path = std::env::var("PATH").unwrap_or_default();
        let mut parts: Vec<&str> = path.split(':').filter(|p| !p.is_empty()).collect();
        for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
            if !parts.contains(&extra) {
                parts.push(extra);
            }
        }
        out.push(("PATH".into(), parts.join(":")));
    }
    out
}

/// Opens `ssh args` in a separate terminal window: a new console on Windows (Windows Terminal
/// if it is the default), whatever app opens `.command` files (Terminal) on macOS.
pub fn open_external(program: &Path, args: &[String]) -> AppResult<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        let mut cmd = std::process::Command::new(program);
        cmd.args(args).creation_flags(CREATE_NEW_CONSOLE);
        if let Some(home) = dirs::home_dir() {
            cmd.current_dir(home);
        }
        cmd.spawn()?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let script = std::env::temp_dir().join(format!(
            "ridgessh-{}.command",
            uuid::Uuid::new_v4().simple()
        ));
        write_command_file(&script, program, args)?;
        let status = std::process::Command::new("/usr/bin/open")
            .arg(&script)
            .status()?;
        if !status.success() {
            let _ = std::fs::remove_file(&script);
            return Err(AppError::Other("could not open a terminal window".into()));
        }
        Ok(())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (program, args);
        Err(AppError::Other(
            "external terminal is not supported on this platform".into(),
        ))
    }
}

/// A shell script that deletes itself and then becomes `program args`.
#[cfg(unix)]
fn write_command_file(path: &Path, program: &Path, args: &[String]) -> AppResult<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let line = std::iter::once(program.to_string_lossy().into_owned())
        .chain(args.iter().cloned())
        .map(|a| sh_quote(&a))
        .collect::<Vec<_>>()
        .join(" ");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(path)?;
    write!(f, "#!/bin/sh\nrm -f \"$0\"\ncd ~ && clear\nexec {line}\n")?;
    Ok(())
}

/// POSIX single quoting, safe for any text without NUL.
#[cfg(unix)]
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(all(test, unix))]
mod unix_tests {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use super::*;

    #[test]
    fn command_file_runs_the_exact_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("t.command");
        let out = dir.path().join("out");
        // `printf` stands in for ssh: every argument must arrive unchanged.
        let tricky = ["it's", "two words", "$HOME", "`x`", "a\"b", ";", ""];
        let mut args = vec!["%s|".to_string()];
        args.extend(tricky.iter().map(|s| s.to_string()));
        write_command_file(&script, Path::new("printf"), &args).unwrap();
        let status = std::process::Command::new("/bin/sh")
            .arg(&script)
            .env("TERM", "dumb")
            .stdout(std::fs::File::create(&out).unwrap())
            .status()
            .unwrap();
        assert!(status.success());
        let printed = std::fs::read_to_string(&out).unwrap();
        assert!(
            printed.ends_with(&format!("{}|", tricky.join("|"))),
            "{printed:?}"
        );
        assert!(!script.exists(), "the script removes itself");
    }

    #[test]
    fn route_holds_input_until_a_process_is_attached() {
        let manager = Arc::new(TerminalManager::default());
        manager.open_route("r", 80, 24).unwrap();
        // Typed (or ConPTY's cursor reply) before the step's process exists: held.
        manager.write("r", b"hello\r").unwrap();
        let (data_tx, data_rx) = mpsc::channel::<Vec<u8>>();
        let (done_tx, done_rx) = mpsc::channel();
        let id = manager
            .spawn_with(
                SpawnRequest {
                    program: &PathBuf::from("/bin/sh"),
                    args: vec!["-c".into(), "read l; printf 'got:%s' \"$l\"".into()],
                    cols: 80,
                    rows: 24,
                },
                Box::new(move |b| {
                    let _ = data_tx.send(b.to_vec());
                    true
                }),
                move |code| done_tx.send(code).unwrap(),
            )
            .unwrap();
        manager.set_route("r", Some(&id)).unwrap();
        assert_eq!(
            done_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            Some(0)
        );
        let mut seen = Vec::new();
        while let Ok(chunk) = data_rx.recv_timeout(Duration::from_millis(300)) {
            seen.extend(chunk);
        }
        assert!(String::from_utf8_lossy(&seen).contains("got:hello"));

        manager.set_route("r", None).unwrap();
        manager.close("r").unwrap();
        assert!(manager.route_state("r").1, "closing a route cancels it");
    }

    #[test]
    fn pty_session_reports_output_and_exit_code() {
        let manager = Arc::new(TerminalManager::default());
        let (data_tx, data_rx) = mpsc::channel::<Vec<u8>>();
        let (done_tx, done_rx) = mpsc::channel();
        let on_data = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                let _ = data_tx.send(bytes);
            }
            Ok(())
        });
        manager
            .spawn(
                SpawnRequest {
                    program: &PathBuf::from("/bin/sh"),
                    args: vec!["-c".into(), "printf ridge-pty-ok; exit 3".into()],
                    cols: 80,
                    rows: 24,
                },
                on_data,
                Channel::new(|_| Ok(())),
                move |code| done_tx.send(code).unwrap(),
            )
            .unwrap();
        assert_eq!(
            done_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            Some(3)
        );
        let mut seen = Vec::new();
        while let Ok(chunk) = data_rx.recv_timeout(Duration::from_millis(300)) {
            seen.extend(chunk);
        }
        assert!(String::from_utf8_lossy(&seen).contains("ridge-pty-ok"));
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    /// Collects output and, like a real terminal, answers ConPTY's startup cursor-position
    /// query (`ESC[6n`) — ConPTY blocks the child until it gets a reply.
    fn terminal_like_channel(
        manager: Arc<TerminalManager>,
        session: Arc<Mutex<Option<String>>>,
        tx: mpsc::Sender<Vec<u8>>,
    ) -> Channel {
        Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                if bytes.windows(4).any(|w| w == b"\x1b[6n") {
                    // The first query can arrive before `spawn` returned the id.
                    for _ in 0..100 {
                        if let Some(id) = session.lock().unwrap().clone() {
                            let _ = manager.write(&id, b"\x1b[1;1R");
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
                let _ = tx.send(bytes);
            }
            Ok(())
        })
    }

    #[test]
    fn runs_process_in_pty_and_reports_exit() {
        let manager = Arc::new(TerminalManager::default());
        let session = Arc::new(Mutex::new(None));
        let (data_tx, data_rx) = mpsc::channel();
        let (exit_tx, exit_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let on_exit = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                let _ = exit_tx.send(json);
            }
            Ok(())
        });
        let cmd = PathBuf::from(std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into()));
        let id = manager
            .spawn(
                SpawnRequest {
                    program: &cmd,
                    args: vec!["/c".into(), "echo ridge-pty-ok & exit 3".into()],
                    cols: 80,
                    rows: 24,
                },
                terminal_like_channel(manager.clone(), session.clone(), data_tx),
                on_exit,
                move |code| done_tx.send(code).unwrap(),
            )
            .unwrap();
        *session.lock().unwrap() = Some(id);

        let code = match done_rx.recv_timeout(Duration::from_secs(15)) {
            Ok(c) => c,
            Err(_) => {
                let output: Vec<u8> = data_rx.try_iter().flatten().collect();
                panic!(
                    "process did not exit; output: {:?}",
                    String::from_utf8_lossy(&output)
                );
            }
        };
        assert_eq!(code, Some(3));
        let exit = exit_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(exit.contains("\"exitCode\":3"), "{exit}");
        let output: Vec<u8> = data_rx.try_iter().flatten().collect();
        assert!(String::from_utf8_lossy(&output).contains("ridge-pty-ok"));
        assert_eq!(manager.count(), 0, "session removed after exit");
    }

    #[test]
    fn real_ssh_runs_in_pty_and_reports_failure() {
        let Ok(ssh) = super::ssh_args::find_tool("ssh") else {
            return;
        };
        let manager = Arc::new(TerminalManager::default());
        let session = Arc::new(Mutex::new(None));
        let (data_tx, data_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let id = manager
            .spawn(
                SpawnRequest {
                    program: &ssh,
                    // -F none: don't read the user's config; .invalid never resolves.
                    args: [
                        "-F",
                        "none",
                        "-o",
                        "ConnectTimeout=3",
                        "--",
                        "ridgessh-selftest.invalid",
                    ]
                    .map(String::from)
                    .to_vec(),
                    cols: 80,
                    rows: 24,
                },
                terminal_like_channel(manager.clone(), session.clone(), data_tx),
                Channel::new(|_| Ok(())),
                move |code| done_tx.send(code).unwrap(),
            )
            .unwrap();
        *session.lock().unwrap() = Some(id);
        let code = done_rx
            .recv_timeout(Duration::from_secs(20))
            .expect("ssh should exit");
        assert_eq!(code, Some(255));
        let output: Vec<u8> = data_rx.try_iter().flatten().collect();
        let text = String::from_utf8_lossy(&output).to_lowercase();
        assert!(text.contains("ridgessh-selftest.invalid"), "{text}");
    }

    #[test]
    fn close_kills_a_running_session() {
        let manager = Arc::new(TerminalManager::default());
        let session = Arc::new(Mutex::new(None));
        let (data_tx, _data_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let cmd = PathBuf::from(std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into()));
        let id = manager
            .spawn(
                SpawnRequest {
                    program: &cmd,
                    args: vec!["/k".into()],
                    cols: 80,
                    rows: 24,
                },
                terminal_like_channel(manager.clone(), session.clone(), data_tx),
                Channel::new(|_| Ok(())),
                move |code| done_tx.send(code).unwrap(),
            )
            .unwrap();
        *session.lock().unwrap() = Some(id.clone());
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(manager.count(), 1);
        manager.close(&id).unwrap();
        done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("killed process should exit");
        assert_eq!(manager.count(), 0);
    }
}
