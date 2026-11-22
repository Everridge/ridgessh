//! Key pairs for new hosts, installing a public key with a password login, and removing a
//! server's stale entries from known_hosts.
//!
//! Everything goes through the system OpenSSH tools (`ssh-keygen`, `ssh`), like the rest of
//! the app. Passwords are typed at ssh's own prompt in a terminal tab; RidgeSSH never sees them.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rusqlite::Transaction;

use crate::db::hosts::{self, Host, HostInput};
use crate::error::{AppError, AppResult};
use crate::history::Source;
use crate::terminal::ssh_args::{find_tool, posix_quote};
use crate::transfer::keys::{inside_ssh_dir, resolve};

#[cfg(test)]
mod tests;

/// A child process without a console window (Windows) and with no input.
fn quiet(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn pub_path(path: &Path) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(".pub");
    PathBuf::from(p)
}

/// Where a new key for `identity_file` goes: inside `~/.ssh`, with nothing there yet.
fn new_key_target(identity_file: &str, home: &Path) -> AppResult<PathBuf> {
    let path = resolve(identity_file, home)
        .filter(|p| inside_ssh_dir(p, home))
        .ok_or_else(|| {
            AppError::Validation(format!(
                "a new key must go inside ~/.ssh (got {identity_file})"
            ))
        })?;
    for p in [path.clone(), pub_path(&path)] {
        if p.symlink_metadata().is_ok() {
            return Err(AppError::Conflict(format!(
                "{} already exists. Choose another identity file name.",
                p.display()
            )));
        }
    }
    Ok(path)
}

/// Generates an ed25519 key pair (no passphrase) at `identity_file`, which must be a new file
/// inside `~/.ssh`. Returns the public key line.
pub fn generate(identity_file: &str, comment: &str, home: &Path) -> AppResult<String> {
    if comment.chars().any(char::is_control) {
        return Err(AppError::Validation("invalid key comment".into()));
    }
    let path = new_key_target(identity_file, home)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let keygen = find_tool("ssh-keygen")?;
    let out = quiet(&keygen)
        .args(["-q", "-t", "ed25519", "-N", "", "-C", comment, "-f"])
        .arg(&path)
        .output()?;
    if !out.status.success() {
        return Err(AppError::Other(format!(
            "ssh-keygen failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let public = std::fs::read_to_string(pub_path(&path))?;
    Ok(public.trim().to_string())
}

/// Creates the host and generates its key in one go. The key is written last, inside the
/// transaction: if that fails the host is not saved, and if the host is refused no key is
/// written.
pub fn create_host_with_key(
    tx: &Transaction,
    input: HostInput,
    home: &Path,
) -> AppResult<(Host, String)> {
    let host = hosts::create(tx, input, Source::User)?;
    let identity_file = host
        .identity_file
        .clone()
        .ok_or_else(|| AppError::Validation("enter the identity file for the new key".into()))?;
    let public_key = generate(&identity_file, &format!("{}@ridgessh", host.alias), home)?;
    Ok((host, public_key))
}

/// Reads and checks the public key next to `identity_file` (`<identity_file>.pub`).
pub fn read_public_key(identity_file: &str, home: &Path) -> AppResult<String> {
    let path = resolve(identity_file, home).ok_or_else(|| {
        AppError::Validation(format!(
            "only absolute and ~/ identity files are supported (got {identity_file})"
        ))
    })?;
    let pub_file = pub_path(&path);
    let meta = std::fs::metadata(&pub_file)
        .map_err(|_| AppError::NotFound(format!("{} (the public key)", pub_file.display())))?;
    if meta.len() > 16 * 1024 {
        return Err(AppError::Validation(format!(
            "{} is not a public key",
            pub_file.display()
        )));
    }
    let key = std::fs::read_to_string(&pub_file)?.trim().to_string();
    check_public_key(&key)?;
    Ok(key)
}

/// One `authorized_keys` line: a known key type, no control characters.
pub fn check_public_key(key: &str) -> AppResult<()> {
    let known = [
        "ssh-ed25519 ",
        "ssh-rsa ",
        "ecdsa-sha2-",
        "sk-ssh-ed25519",
        "sk-ecdsa-",
    ];
    if !known.iter().any(|k| key.starts_with(k)) || key.chars().any(char::is_control) {
        return Err(AppError::Validation("not an OpenSSH public key".into()));
    }
    Ok(())
}

/// The remote command (one argv element after the destination) that adds `public_key` to
/// `~/.ssh/authorized_keys` unless it is already there, and fixes the permissions. One line, so
/// any login shell passes it to `sh` unchanged.
pub fn install_command(public_key: &str) -> AppResult<String> {
    check_public_key(public_key)?;
    let inner = [
        "umask 077".to_string(),
        format!("k={}", posix_quote(public_key)),
        r#"d="$HOME/.ssh""#.into(),
        r#"f="$d/authorized_keys""#.into(),
        r#"mkdir -p "$d" && touch "$f" || exit 1"#.into(),
        // `$(…)` drops a trailing newline, so this is non-empty only when one is missing.
        r#"if [ -s "$f" ] && [ -n "$(tail -c 1 "$f")" ]; then echo >> "$f"; fi"#.into(),
        r#"if grep -qxF "$k" "$f"; then echo "The key was already installed for $(whoami)."; else printf '%s\n' "$k" >> "$f" || exit 1; echo "Public key installed for $(whoami)."; fi"#.into(),
        r#"chmod 700 "$d" && chmod 600 "$f" || exit 1"#.into(),
        // SELinux systems need the right context, as ssh-copy-id does.
        r#"if command -v restorecon >/dev/null 2>&1; then restorecon -F "$d" "$f" >/dev/null 2>&1; fi"#.into(),
        "exit 0".into(),
    ]
    .join("; ");
    Ok(format!("sh -c {}", posix_quote(&inner)))
}

/// What `ssh -G` says about the server's entry in known_hosts.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct KnownHostsTarget {
    /// The name the host key is saved under (HostKeyAlias, else HostName).
    pub name: String,
    pub port: u16,
    /// UserKnownHostsFile, as printed (several paths separated by spaces).
    pub files: String,
}

impl KnownHostsTarget {
    /// The known_hosts names to remove: `name`, and `[name]:port` for other ports.
    pub fn names(&self) -> Vec<String> {
        if self.port == 22 {
            vec![self.name.clone()]
        } else {
            vec![format!("[{}]:{}", self.name, self.port), self.name.clone()]
        }
    }
}

/// Parses `ssh -G` output.
pub fn parse_resolved(text: &str) -> Option<KnownHostsTarget> {
    let mut hostname = None;
    let mut alias = None;
    let mut port = 22;
    let mut files = String::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        let value = value.trim();
        match key.to_ascii_lowercase().as_str() {
            "hostname" => hostname = Some(value.to_string()),
            "hostkeyalias" if !value.is_empty() && value != "none" => {
                alias = Some(value.to_string())
            }
            "port" => port = value.parse().unwrap_or(22),
            "userknownhostsfile" => files = value.to_string(),
            _ => {}
        }
    }
    Some(KnownHostsTarget {
        name: alias.or(hostname)?,
        port,
        files,
    })
}

/// The files named in UserKnownHostsFile that exist. The paths are separated by spaces, but a
/// path may contain spaces too (`C:\Users\First Last\…`), so words are joined until they name
/// an existing file.
pub fn known_hosts_files(files: &str, home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut current = String::new();
    for word in files.split(' ').filter(|w| !w.is_empty()) {
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word.trim_matches('"'));
        if current.contains('%') {
            current.clear();
            continue;
        }
        let path = if current.starts_with('~') {
            resolve(&current, home)
        } else {
            Some(PathBuf::from(&current))
        };
        if let Some(p) = path.filter(|p| p.is_file()) {
            out.push(p);
            current.clear();
        }
    }
    out
}

/// Removes the host's entries from the user's known_hosts files (`ssh-keygen -R`, which keeps
/// the previous file as `known_hosts.old`). Returns the names that were found and removed.
pub fn forget_host_key(
    ssh: &Path,
    options: &[String],
    destination: &str,
    home: &Path,
) -> AppResult<Vec<String>> {
    let out = quiet(ssh)
        .arg("-G")
        .args(options)
        .arg(destination)
        .output()?;
    if !out.status.success() {
        return Err(AppError::Other(format!(
            "could not resolve {destination}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let target = parse_resolved(&String::from_utf8_lossy(&out.stdout))
        .ok_or_else(|| AppError::Other(format!("could not resolve {destination}")))?;
    let keygen = find_tool("ssh-keygen")?;
    remove_entries(
        &keygen,
        &target.names(),
        &known_hosts_files(&target.files, home),
    )
}

/// `ssh-keygen -R <name> -f <file>` for every name and file.
pub fn remove_entries(
    keygen: &Path,
    names: &[String],
    files: &[PathBuf],
) -> AppResult<Vec<String>> {
    let mut removed = Vec::new();
    for file in files {
        for name in names {
            // Option-like names cannot come from a validated host; refuse them anyway.
            if name.starts_with('-') {
                continue;
            }
            let out = quiet(keygen)
                .arg("-R")
                .arg(name)
                .arg("-f")
                .arg(file)
                .output()?;
            if !out.status.success() {
                return Err(AppError::Other(format!(
                    "ssh-keygen -R {name} failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                )));
            }
            // "# Host <name> found: line N" for each removed entry.
            if String::from_utf8_lossy(&out.stdout).contains(" found: ") && !removed.contains(name)
            {
                removed.push(name.clone());
            }
        }
    }
    Ok(removed)
}
