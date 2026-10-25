//! Remote folder browser: lists one folder over ssh, for picking a path.
//!
//! This is not a terminal tab, so it cannot answer prompts: it runs with `BatchMode=yes` and
//! works with key-based logins (or an agent). Anything else fails fast with an explanation,
//! and the path can still be typed.

use std::path::Path;
use std::process::{Command, Stdio};

use serde::Serialize;

use super::{RESOLVE, remote};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    /// The folder's absolute path on the server.
    pub path: String,
    /// Folders first, then files, by name.
    pub entries: Vec<Entry>,
    /// More than [`MAX_ENTRIES`] entries; only the first ones are shown.
    pub truncated: bool,
}

pub const MAX_ENTRIES: usize = 2000;

/// Prints the absolute path, then one entry per line (`ls -p`: folders end in `/`).
/// An empty path means the home folder.
const BROWSE: &str = r#"[ -n "$p" ] || p=$h
cd -- "$p" 2>/dev/null || { echo "Cannot open $p" >&2; exit 10; }
pwd
ls -1Ap 2>/dev/null | head -n 2001
"#;

pub fn browse_command(path: &str) -> AppResult<String> {
    let p = path.trim();
    if p.len() > 4096 || p.chars().any(char::is_control) {
        return Err(AppError::Validation("not a usable path".into()));
    }
    Ok(remote(&format!("{RESOLVE}{BROWSE}"), &[p]))
}

pub fn parse(stdout: &str) -> AppResult<Listing> {
    let mut lines = stdout.lines().map(|l| l.trim_end_matches('\r'));
    let path = lines
        .next()
        .filter(|p| p.starts_with('/'))
        .ok_or_else(|| AppError::Other("the server sent an unexpected listing".into()))?
        .to_string();
    let mut entries: Vec<Entry> = lines
        .filter(|l| !l.is_empty())
        .map(|l| match l.strip_suffix('/') {
            Some(name) => Entry {
                name: name.to_string(),
                dir: true,
            },
            None => Entry {
                name: l.to_string(),
                dir: false,
            },
        })
        .collect();
    let truncated = entries.len() > MAX_ENTRIES;
    entries.truncate(MAX_ENTRIES);
    entries.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(Listing {
        path,
        entries,
        truncated,
    })
}

/// Runs the listing with `ssh <options> -o BatchMode=yes <destination>` (no console window).
pub fn run(ssh: &Path, options: &[String], destination: &str, path: &str) -> AppResult<Listing> {
    let mut cmd = Command::new(ssh);
    cmd.args(options)
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"])
        .arg(destination)
        .arg(browse_command(path)?)
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output()?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    let last = stderr
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    match out.status.code() {
        Some(0) => parse(&String::from_utf8_lossy(&out.stdout)),
        Some(10) => Err(AppError::NotFound(last.trim().to_string())),
        Some(255) => Err(AppError::Other(format!(
            "could not connect without a prompt ({}). Browsing needs a key-based login; type the path instead.",
            last.trim()
        ))),
        _ => Err(AppError::Other(format!(
            "listing failed: {}",
            if last.is_empty() {
                "unknown error"
            } else {
                last.trim()
            }
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_listings() {
        let l = parse("/var/www\r\nsite/\nREADME\n.env\nassets/\n").unwrap();
        assert_eq!(l.path, "/var/www");
        assert_eq!(
            l.entries
                .iter()
                .map(|e| (e.name.as_str(), e.dir))
                .collect::<Vec<_>>(),
            [
                ("assets", true),
                ("site", true),
                (".env", false),
                ("README", false)
            ]
        );
        assert!(!l.truncated);
        assert!(parse("").is_err());
        assert!(parse("relative\n").is_err());
    }

    /// The script against the local shell, with a fake home.
    #[test]
    fn lists_a_folder_with_sh() {
        let Ok(sh) = which::which("sh") else { return };
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join("my site")).unwrap();
        std::fs::write(home.path().join("notes.txt"), b"x").unwrap();
        let run = |path: &str| {
            Command::new(&sh)
                .arg("-c")
                .arg(browse_command(path).unwrap())
                .env("HOME", home.path())
                .output()
                .unwrap()
        };
        for path in ["", "~"] {
            let out = run(path);
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let l = parse(&String::from_utf8_lossy(&out.stdout)).unwrap();
            assert!(l.entries.contains(&Entry {
                name: "my site".into(),
                dir: true
            }));
            assert!(l.entries.iter().any(|e| e.name == "notes.txt" && !e.dir));
        }
        assert_eq!(run("~/nope").status.code(), Some(10));
    }
}
