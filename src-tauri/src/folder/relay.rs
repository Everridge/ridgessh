//! Server → server without a copy on this PC (M11 phase 4): the source server's archive is
//! read by one ssh process and written into the destination's inbox by another, and this
//! app copies between the two pipes, counting bytes and hashing as it goes.
//!
//! The pipes carry the archive, so neither ssh has a terminal to prompt in: both run with
//! `BatchMode=yes` and need key-based logins (or an agent). Without one they fail fast, and
//! the transfer can run via this PC instead. `sudo` still works, because the steps that use
//! it (zip, unpack) run in the tab as usual; the archive itself belongs to the login user.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::runner::Conn;
use super::{Manifest, parse_manifest, read_sum_command, receive_command, send_command};
use crate::error::{AppError, AppResult};

pub enum Relayed {
    /// Everything the source sent reached the destination.
    Done {
        bytes: u64,
        sha256: String,
    },
    Cancelled,
}

/// `ssh <options> -o BatchMode=yes <destination> <command>`, without a console window.
fn batch(ssh: &Path, conn: &Conn, remote_command: &str) -> Command {
    let mut cmd = Command::new(ssh);
    cmd.args(&conn.options)
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=15"])
        .arg(&conn.destination)
        .arg(remote_command);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn last_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn no_prompt(conn: &Conn, why: &str) -> AppError {
    AppError::Other(format!(
        "could not connect to {} without a prompt ({why}). Streaming directly needs key-based \
         logins on both servers; turn off \"Stream directly\" to copy via this PC, where \
         password prompts work",
        conn.destination
    ))
}

/// Explains a failed ssh process: 255 is ssh itself (connecting, logging in).
fn ssh_failed(conn: &Conn, code: Option<i32>, stderr: &[u8], what: &str) -> AppError {
    let why = last_line(stderr);
    match code {
        Some(255) => no_prompt(conn, if why.is_empty() { "ssh failed" } else { &why }),
        _ => AppError::Other(format!(
            "{what} on {} failed{}",
            conn.destination,
            if why.is_empty() {
                String::new()
            } else {
                format!(": {why}")
            }
        )),
    }
}

/// Reads the archive's `.sum` from the source server (this also tests the login).
pub fn fetch_manifest(ssh: &Path, from: &Conn, archive: &str) -> AppResult<Manifest> {
    let out = batch(ssh, from, &read_sum_command(archive)?)
        .stdin(Stdio::null())
        .output()?;
    if !out.status.success() {
        return Err(ssh_failed(
            from,
            out.status.code(),
            &out.stderr,
            "reading the archive summary",
        ));
    }
    parse_manifest(&String::from_utf8_lossy(&out.stdout))
}

/// Tests the login on the destination server before anything is streamed.
pub fn probe(ssh: &Path, to: &Conn) -> AppResult<()> {
    let out = batch(ssh, to, "true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(ssh_failed(to, out.status.code(), &out.stderr, "connecting"))
    }
}

fn read_all(mut stderr: ChildStderr) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = stderr.read_to_end(&mut buf);
    buf
}

/// Streams `archive` on `from` into `inbox` on `to`. `progress` gets the bytes so far about
/// twice a second; `cancelled` is polled, and stops both processes when it turns true.
pub fn relay(
    ssh: &Path,
    from: &Conn,
    archive: &str,
    to: &Conn,
    inbox: &str,
    cancelled: &(dyn Fn() -> bool + Sync),
    progress: &mut dyn FnMut(u64),
) -> AppResult<Relayed> {
    use sha2::{Digest, Sha256};

    let mut sender = batch(ssh, from, &send_command(archive)?)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut receiver = match batch(ssh, to, &receive_command(inbox)?)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = sender.kill();
            let _ = sender.wait();
            return Err(e.into());
        }
    };
    let (Some(mut source), Some(mut sink), Some(err_from), Some(err_to)) = (
        sender.stdout.take(),
        receiver.stdin.take(),
        sender.stderr.take(),
        receiver.stderr.take(),
    ) else {
        return Err(AppError::Other("could not connect the ssh pipes".into()));
    };
    let children: Mutex<(Child, Child)> = Mutex::new((sender, receiver));
    let kill = || {
        if let Ok(mut c) = children.lock() {
            let _ = c.0.kill();
            let _ = c.1.kill();
        }
    };
    let finished = AtomicBool::new(false);
    let stopped = AtomicBool::new(false);

    std::thread::scope(|s| {
        let err_from = s.spawn(move || read_all(err_from));
        let err_to = s.spawn(move || read_all(err_to));
        let watcher = s.spawn(|| {
            loop {
                if cancelled() {
                    stopped.store(true, Ordering::Relaxed);
                    kill();
                    return;
                }
                if finished.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        });

        let mut hash = Sha256::new();
        let mut bytes = 0u64;
        let mut buf = vec![0u8; 256 * 1024];
        let mut shown = Instant::now();
        // Which side broke the stream, if one did.
        let mut broke: Option<bool> = None;
        loop {
            let n = match source.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    broke = Some(true);
                    break;
                }
            };
            if sink.write_all(&buf[..n]).is_err() {
                broke = Some(false);
                break;
            }
            hash.update(&buf[..n]);
            bytes += n as u64;
            if shown.elapsed() >= Duration::from_millis(500) {
                progress(bytes);
                shown = Instant::now();
            }
        }
        progress(bytes);
        if broke.is_some() {
            kill();
        }
        // End of the archive: the destination's `cat` finishes and ssh exits.
        drop(sink);
        drop(source);
        // Polled, so the lock stays free for a cancel while a process takes its time.
        let wait = |second: bool| loop {
            match children.lock() {
                Ok(mut c) => {
                    let child = if second { &mut c.1 } else { &mut c.0 };
                    match child.try_wait() {
                        Ok(Some(status)) => return status.code(),
                        Ok(None) => {}
                        Err(_) => return None,
                    }
                }
                Err(_) => return None,
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let (code_from, code_to) = (wait(false), wait(true));
        finished.store(true, Ordering::Relaxed);
        let _ = watcher.join();
        let err_from = err_from.join().unwrap_or_default();
        let err_to = err_to.join().unwrap_or_default();
        if stopped.load(Ordering::Relaxed) {
            return Ok(Relayed::Cancelled);
        }
        // The side that failed first explains it best; a killed partner fails too.
        let from_failed = || ssh_failed(from, code_from, &err_from, "reading the archive");
        let to_failed = || ssh_failed(to, code_to, &err_to, "saving the archive");
        match broke {
            Some(false) => return Err(to_failed()),
            Some(true) => return Err(from_failed()),
            None => {}
        }
        if code_from != Some(0) {
            return Err(from_failed());
        }
        if code_to != Some(0) {
            return Err(to_failed());
        }
        Ok(Relayed::Done {
            bytes,
            sha256: hex::encode(hash.finalize()),
        })
    })
}
