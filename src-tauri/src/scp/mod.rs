//! File transfers with the system `scp` (M7).
//!
//! A transfer runs in a terminal tab like a session, so password prompts, host-key questions
//! and scp's own progress meter work as in a console. Windows OpenSSH 9.x scp uses the SFTP
//! protocol, so remote paths are not interpreted by a remote shell.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::db::hosts::Host;
use crate::error::{AppError, AppResult};
use crate::terminal::ssh_args::host_options;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Upload,
    Download,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferSpec {
    pub host_id: String,
    pub direction: Direction,
    /// Upload: the files/folders to send. Download: one local folder to save into.
    pub local: Vec<String>,
    /// Upload: remote target folder or file. Download: the remote file or folder.
    pub remote: String,
    /// Copy folders (`-r`). Always on for uploads that include a folder.
    #[serde(default)]
    pub recursive: bool,
    /// Connect as this user (`user@alias`, see `target_args`).
    #[serde(default)]
    pub user: Option<String>,
}

fn check_remote(remote: &str) -> AppResult<&str> {
    let r = remote.trim();
    if r.is_empty() || r.len() > 4096 || r.chars().any(char::is_control) {
        return Err(AppError::Validation("enter a valid remote path".into()));
    }
    Ok(r)
}

fn absolute(p: &str) -> AppResult<PathBuf> {
    let path = PathBuf::from(p);
    if !path.is_absolute() || p.chars().any(char::is_control) {
        return Err(AppError::Validation(format!(
            "{p} is not an absolute local path"
        )));
    }
    Ok(path)
}

/// The scp argument vector for `spec` (no shell is involved).
pub fn scp_args(host: &Host, spec: &TransferSpec) -> AppResult<Vec<String>> {
    let remote = check_remote(&spec.remote)?;
    let (mut args, destination) = host_options(host, spec.user.as_deref())?;
    let target = format!("{destination}:{remote}");
    match spec.direction {
        Direction::Upload => {
            if spec.local.is_empty() {
                return Err(AppError::Validation("choose files to upload".into()));
            }
            let paths = spec
                .local
                .iter()
                .map(|p| absolute(p))
                .collect::<AppResult<Vec<_>>>()?;
            for p in &paths {
                if !p.exists() {
                    return Err(AppError::NotFound(p.display().to_string()));
                }
            }
            if spec.recursive || paths.iter().any(|p| p.is_dir()) {
                args.push("-r".into());
            }
            args.push("--".into());
            args.extend(paths.iter().map(|p| p.display().to_string()));
            args.push(target);
        }
        Direction::Download => {
            let [dir] = spec.local.as_slice() else {
                return Err(AppError::Validation("choose one local folder".into()));
            };
            let dir = absolute(dir)?;
            if !dir.is_dir() {
                return Err(AppError::NotFound(dir.display().to_string()));
            }
            if spec.recursive {
                args.push("-r".into());
            }
            args.push("--".into());
            args.push(target);
            args.push(dir.display().to_string());
        }
    }
    Ok(args)
}

/// Short description for tab titles and the activity log.
pub fn summary(spec: &TransferSpec) -> String {
    let name = |p: &str| {
        Path::new(p)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.to_string())
    };
    match spec.direction {
        Direction::Upload => match spec.local.as_slice() {
            [one] => format!("↑ {} → {}", name(one), spec.remote.trim()),
            many => format!("↑ {} items → {}", many.len(), spec.remote.trim()),
        },
        Direction::Download => format!("↓ {}", spec.remote.trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> Host {
        crate::terminal::ssh_args::tests_support::host()
    }

    #[test]
    fn upload_and_download_args() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a b.txt");
        std::fs::write(&file, "x").unwrap();
        let mut h = host();
        h.config_snapshot = Some(crate::db::hosts::Connection {
            alias: "web".into(),
            ..Default::default()
        });

        let up = TransferSpec {
            host_id: h.id.clone(),
            direction: Direction::Upload,
            local: vec![file.display().to_string(), dir.path().display().to_string()],
            remote: " /srv/my app/ ".into(),
            recursive: false,
            user: None,
        };
        let args = scp_args(&h, &up).unwrap();
        assert_eq!(
            args,
            [
                "-r".to_string(),
                "--".into(),
                file.display().to_string(),
                dir.path().display().to_string(),
                "web:/srv/my app/".into()
            ]
        );

        let down = TransferSpec {
            direction: Direction::Download,
            local: vec![dir.path().display().to_string()],
            remote: "/var/log/syslog".into(),
            ..up.clone()
        };
        assert_eq!(
            scp_args(&h, &down).unwrap(),
            [
                "--".to_string(),
                "web:/var/log/syslog".into(),
                dir.path().display().to_string()
            ]
        );
        assert_eq!(summary(&down), "↓ /var/log/syslog");
    }

    #[test]
    fn rejects_bad_input() {
        let h = host();
        let spec = |local: Vec<String>, remote: &str, direction| TransferSpec {
            host_id: h.id.clone(),
            direction,
            local,
            remote: remote.into(),
            recursive: false,
            user: None,
        };
        assert!(scp_args(&h, &spec(vec![], "/tmp", Direction::Upload)).is_err());
        assert!(
            scp_args(
                &h,
                &spec(vec!["relative.txt".into()], "/tmp", Direction::Upload)
            )
            .is_err()
        );
        assert!(
            scp_args(
                &h,
                &spec(
                    vec![r"C:\definitely\missing".into()],
                    "/tmp",
                    Direction::Upload
                )
            )
            .is_err()
        );
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().display().to_string();
        assert!(scp_args(&h, &spec(vec![d.clone()], "", Direction::Download)).is_err());
        assert!(scp_args(&h, &spec(vec![d.clone()], "a\nb", Direction::Download)).is_err());
        assert!(scp_args(&h, &spec(vec![d.clone(), d], "/x", Direction::Download)).is_err());
    }
}
