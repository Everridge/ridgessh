//! Folder transfers (M11): a folder is zipped where it is (on a server, or on this PC),
//! carried over with a checksum check, and unpacked where it goes; temporary archives are
//! removed at the end. Server → PC, PC → server and server → PC → server.
//!
//! Each step is one ssh/scp process in the tab's PTY (see [`runner`]), so password and
//! host-key prompts work. Remote steps are fixed `sh -c` scripts; everything that varies (the
//! folder path, archive names) is passed as quoted positional arguments, never spliced into
//! the script. Steps report success through their exit codes, and the archive's checksum
//! travels in a small `.sum` file (downloads) or as an argument to the unpack script
//! (uploads), so nothing depends on parsing terminal output.

pub mod browse;
pub mod local;
pub mod recipes;
pub mod relay;
pub mod runner;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::terminal::ssh_args::posix_quote;

/// One side of a transfer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Endpoint {
    /// A server. As a source, `path` is the folder; as a destination, the folder that
    /// receives it (`path/<name>`). Absolute, `~/…`, or relative to the login directory.
    #[serde(rename_all = "camelCase")]
    Remote {
        host_id: String,
        /// Connect as this user (`user@alias`, see `target_args`).
        #[serde(default)]
        user: Option<String>,
        path: String,
    },
    /// This PC: the folder itself (source), or the folder that receives `<name>.zip` and
    /// `<name>/` (destination). Absolute.
    #[serde(rename_all = "camelCase")]
    Local { path: String },
}

impl Endpoint {
    pub fn host_id(&self) -> Option<&str> {
        match self {
            Endpoint::Remote { host_id, .. } => Some(host_id),
            Endpoint::Local { .. } => None,
        }
    }

    pub fn path(&self) -> &str {
        match self {
            Endpoint::Remote { path, .. } | Endpoint::Local { path } => path.trim(),
        }
    }
}

/// What to do when the destination already has a folder of that name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Existing {
    /// Move it aside to `name.bak-<date>` first.
    #[default]
    Backup,
    /// Unpack over it: same-named files are replaced, others are kept.
    Merge,
    /// Stop without changing anything.
    Stop,
}

impl Existing {
    fn as_arg(self) -> &'static str {
        match self {
            Existing::Backup => "backup",
            Existing::Merge => "merge",
            Existing::Stop => "stop",
        }
    }
}

/// What to transfer and what to do with it.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSpec {
    pub source: Endpoint,
    pub destination: Endpoint,
    /// This PC as destination: unpack the archive after downloading.
    #[serde(default)]
    pub unpack: bool,
    /// This PC as destination: keep the `.zip` after unpacking.
    #[serde(default = "yes")]
    pub keep_zip: bool,
    /// Server → server: also keep the archive in this local folder.
    #[serde(default)]
    pub keep_copy: Option<String>,
    /// Server → server: stream the archive from one server to the other through this app
    /// instead of saving it here first. Needs logins without prompts (keys or an agent).
    #[serde(default)]
    pub direct: bool,
    #[serde(default)]
    pub existing: Existing,
    /// Server as destination: `chown -R` the unpacked folder (`user` or `user:group`).
    #[serde(default)]
    pub owner: Option<String>,
    /// Names or relative paths to leave out (`node_modules`, `*.log`, `storage/cache`).
    #[serde(default)]
    pub excludes: Vec<String>,
    /// Check and zip the source folder as root (folders only root can read).
    #[serde(default)]
    pub sudo_source: bool,
    /// Check, unpack and set the owner at the destination as root.
    #[serde(default)]
    pub sudo_dest: bool,
    /// Runs on the source server before zipping (e.g. dump a database, stop a service).
    #[serde(default)]
    pub before: Option<Hook>,
    /// Runs on the destination server after unpacking (e.g. restart a service).
    #[serde(default)]
    pub after: Option<Hook>,
}

/// A script around a transfer: one from the library, or a command line.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hook {
    #[serde(default)]
    pub script_id: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
}

impl Hook {
    fn check(&self, which: &str) -> AppResult<()> {
        match (&self.script_id, self.command.as_deref().map(str::trim)) {
            (Some(_), None) => Ok(()),
            (None, Some(c)) if !c.is_empty() && c.len() <= 4000 && !c.contains('\0') => Ok(()),
            _ => Err(AppError::Validation(format!(
                "choose a script or enter a command for \"{which}\""
            ))),
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StepKind {
    /// The source folder (on the server, or on this PC).
    Check,
    /// The destination server.
    CheckDest,
    Pack,
    PackLocal,
    Download,
    Unpack,
    Upload,
    /// Server → server without a copy on this PC (see [`relay`]).
    Relay,
    UnpackRemote,
    SetOwner,
    /// The source server's script, before zipping.
    Before,
    /// The destination server's script, after unpacking.
    After,
    Cleanup,
}

impl StepKind {
    pub fn label(self) -> &'static str {
        match self {
            StepKind::Check => "Check source",
            StepKind::CheckDest => "Check destination",
            StepKind::Pack => "Zip on server",
            StepKind::PackLocal => "Zip on this PC",
            StepKind::Download => "Download",
            StepKind::Unpack => "Unpack locally",
            StepKind::Upload => "Upload",
            StepKind::Relay => "Stream to destination",
            StepKind::UnpackRemote => "Unpack on server",
            StepKind::SetOwner => "Set owner",
            StepKind::Before => "Script before",
            StepKind::After => "Script after",
            StepKind::Cleanup => "Clean up",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepInfo {
    pub kind: StepKind,
    pub label: &'static str,
}

/// The steps `spec` runs, in order.
pub fn steps(spec: &FolderSpec) -> Vec<StepKind> {
    use Endpoint::{Local, Remote};
    let remote_dest = matches!(spec.destination, Remote { .. });
    let mut out = vec![StepKind::Check];
    if remote_dest {
        out.push(StepKind::CheckDest);
    }
    if spec.before.is_some() {
        out.push(StepKind::Before);
    }
    match spec.source {
        Remote { .. } if remote_dest && spec.direct => {
            out.extend([StepKind::Pack, StepKind::Relay])
        }
        Remote { .. } => out.extend([StepKind::Pack, StepKind::Download]),
        Local { .. } => out.push(StepKind::PackLocal),
    }
    if remote_dest {
        if !out.contains(&StepKind::Relay) {
            out.push(StepKind::Upload);
        }
        out.push(StepKind::UnpackRemote);
        if spec.owner.is_some() {
            out.push(StepKind::SetOwner);
        }
        if spec.after.is_some() {
            out.push(StepKind::After);
        }
    } else if spec.unpack {
        out.push(StepKind::Unpack);
    }
    out.push(StepKind::Cleanup);
    out
}

/// Checks everything that can be checked before connecting anywhere.
pub fn check_spec(spec: &FolderSpec) -> AppResult<()> {
    let local_dir = |p: &str, what: &str| -> AppResult<()> {
        let path = Path::new(p);
        if !path.is_absolute() || !path.is_dir() {
            return Err(AppError::Validation(format!(
                "choose an existing {what} on this PC (not {p:?})"
            )));
        }
        Ok(())
    };
    match &spec.source {
        Endpoint::Remote { path, .. } => {
            check_remote_path(path)?;
        }
        Endpoint::Local { path } => {
            local_dir(path.trim(), "folder to upload")?;
            if Path::new(path.trim()).file_name().is_none() {
                return Err(AppError::Validation(format!(
                    "{path} is not a folder that can be zipped"
                )));
            }
        }
    }
    match &spec.destination {
        Endpoint::Remote { path, .. } => {
            check_remote_dir(path)?;
        }
        Endpoint::Local { path } => {
            if matches!(spec.source, Endpoint::Local { .. }) {
                return Err(AppError::Validation(
                    "copying between two folders on this PC is not supported".into(),
                ));
            }
            local_dir(path.trim(), "folder to save into")?;
        }
    }
    if let Some(dir) = &spec.keep_copy {
        local_dir(dir.trim(), "folder for the local copy")?;
    }
    if let Some(owner) = &spec.owner {
        check_owner(owner)?;
    }
    check_excludes(&spec.excludes)?;
    let remote_src = matches!(spec.source, Endpoint::Remote { .. });
    let remote_dst = matches!(spec.destination, Endpoint::Remote { .. });
    if let Some(h) = &spec.before {
        if !remote_src {
            return Err(AppError::Validation(
                "a script before the transfer runs on the source server".into(),
            ));
        }
        h.check("script before")?;
    }
    if let Some(h) = &spec.after {
        if !remote_dst {
            return Err(AppError::Validation(
                "a script after the transfer runs on the destination server".into(),
            ));
        }
        h.check("script after")?;
    }
    if spec.direct && !(remote_src && remote_dst) {
        return Err(AppError::Validation(
            "streaming directly works between two servers only".into(),
        ));
    }
    if spec.direct && spec.keep_copy.is_some() {
        return Err(AppError::Validation(
            "a streamed copy is not stored on this PC, so it cannot keep a local copy".into(),
        ));
    }
    if (spec.sudo_source && !remote_src) || (spec.sudo_dest && !remote_dst) {
        return Err(AppError::Validation("sudo applies to servers only".into()));
    }
    Ok(())
}

/// `user`, `user:group`, numeric ids too; nothing a shell or `chown` could misread.
pub fn check_owner(owner: &str) -> AppResult<()> {
    let part = |s: &str| {
        !s.is_empty()
            && s.len() <= 32
            && !s.starts_with('-')
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    };
    let ok = match owner.split_once(':') {
        Some((u, g)) => part(u) && part(g),
        None => part(owner),
    };
    if ok {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "{owner:?} is not an owner; use user or user:group"
        )))
    }
}

/// A destination folder on a server (the folder that receives the copy).
pub fn check_remote_dir(path: &str) -> AppResult<&str> {
    let p = path.trim();
    if p.is_empty() || p.len() > 4096 || p.chars().any(char::is_control) {
        return Err(AppError::Validation(
            "enter the destination folder on the server".into(),
        ));
    }
    Ok(p)
}

/// The folder's name as it will appear at the destination, when known before anything ran
/// (`None` for a remote `~`, whose name depends on the server).
pub fn source_name(spec: &FolderSpec) -> Option<String> {
    let p = spec.source.path().trim_end_matches(['/', '\\']);
    let last = p.rsplit(['/', '\\']).next()?;
    (!last.is_empty() && last != "~").then(|| last.to_string())
}

/// The remote path as typed, checked for things no folder path contains.
pub fn check_remote_path(path: &str) -> AppResult<&str> {
    let p = path.trim();
    let last = p.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if p.is_empty() || p.len() > 4096 || p.chars().any(char::is_control) {
        return Err(AppError::Validation(
            "enter the remote folder's path".into(),
        ));
    }
    if p.trim_end_matches('/').is_empty() || last == "." || last == ".." {
        return Err(AppError::Validation(format!(
            "{p} is not a folder that can be zipped; enter its full path"
        )));
    }
    Ok(p)
}

/// The run's server-side archive. Only names of this exact shape are ever deleted.
pub fn remote_archive(run_id: &str) -> String {
    format!("/tmp/ridgessh-{run_id}.zip")
}

/// The run's archive as uploaded to the destination server. A different name from
/// [`remote_archive`], so copying between two folders of one server works too.
pub fn remote_inbox(run_id: &str) -> String {
    format!("/tmp/ridgessh-{run_id}-in.zip")
}

fn is_own_archive(path: &str) -> bool {
    path.strip_prefix("/tmp/ridgessh-")
        .and_then(|r| r.strip_suffix(".zip"))
        .map(|id| id.strip_suffix("-in").unwrap_or(id))
        .is_some_and(|id| id.len() == 12 && id.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Resolves `~` and `~/…` like a shell would (quoting stops the shell from doing it), against
/// the login user's home `$h` (see [`remote`]), which stays right under `sudo`.
const RESOLVE: &str = r#"p=$1
case $p in '~') p=$h ;; '~/'*) p=$h/${p#'~/'} ;; esac
"#;

/// Exit codes of the remote scripts, explained in the tab.
pub const EXIT_NOT_FOUND: u32 = 10;
pub const EXIT_NO_PACKER: u32 = 11;
pub const EXIT_NO_SPACE: u32 = 12;
pub const EXIT_NO_ACCESS: u32 = 13;
/// zip's "some files could not be read" (the archive is still complete otherwise).
pub const EXIT_SKIPPED_FILES: u32 = 18;

const CHECK: &str = r#"name=$(basename -- "$p")
case $name in /|.|..) echo "Choose a folder, not $p" >&2; exit 14 ;; esac
if [ ! -d "$p" ]; then echo "Folder not found: $p" >&2; exit 10; fi
if [ ! -r "$p" ] || [ ! -x "$p" ]; then echo "No permission to read $p" >&2; exit 13; fi
if command -v zip >/dev/null 2>&1; then tool=zip
elif command -v python3 >/dev/null 2>&1; then tool='python3 (zip is not installed)'
else echo 'Neither zip nor python3 is installed on the server. Install zip, for example: sudo apt install zip' >&2; exit 11; fi
size=$(du -sk -- "$p" 2>/dev/null | cut -f1)
free=$(df -Pk /tmp | awk 'NR==2 {print $4}')
echo "Folder:        $p"
echo "Size:          $(( ${size:-0} / 1024 )) MB"
echo "Zip with:      $tool"
echo "Free in /tmp:  $(( ${free:-0} / 1024 )) MB"
if [ -n "$size" ] && [ -n "$free" ] && [ "$free" -lt "$size" ]; then
  echo "Not enough free space in /tmp for the archive (it can need up to $(( size / 1024 )) MB)." >&2; exit 12
fi
"#;

/// Args: folder, archive, python packer, then exclude patterns (see [`exclude_patterns`]).
const PACK: &str = r#"out=$2; py=$3; shift 3
cd -- "$(dirname -- "$p")" || exit 13
name=$(basename -- "$p")
umask 077
rm -f -- "$out" "$out.sum"
if command -v zip >/dev/null 2>&1; then
  if [ $# -gt 0 ]; then zip -r -y -q "$out" "./$name" -x "$@"; else zip -r -y -q "$out" "./$name"; fi; rc=$?
else python3 -c "$py" "$name" "$out" "$@"; rc=$?; fi
if [ $rc -eq 18 ]; then echo 'Some files could not be read and were left out (see above).' >&2
elif [ $rc -ne 0 ]; then rm -f -- "$out"; exit $rc; fi
if command -v sha256sum >/dev/null 2>&1; then sum=$(sha256sum -- "$out" | cut -d' ' -f1)
elif command -v shasum >/dev/null 2>&1; then sum=$(shasum -a 256 "$out" | cut -d' ' -f1)
elif command -v python3 >/dev/null 2>&1; then sum=$(python3 -c 'import hashlib,sys
h = hashlib.sha256()
with open(sys.argv[1], "rb") as f:
    for b in iter(lambda: f.read(1 << 20), b""): h.update(b)
print(h.hexdigest())' "$out")
else sum=-; fi
bytes=$(wc -c < "$out" | tr -d ' ')
printf '%s %s %s\n' "$sum" "$bytes" "$name" > "$out.sum"
# Zipped with sudo: hand the archive to the login user, who downloads and deletes it.
if [ -n "$SUDO_USER" ]; then chown -- "$SUDO_USER" "$out" "$out.sum" || exit 13; fi
echo "Zipped $name: $(( bytes / 1048576 )) MB"
exit $rc
"#;

const CLEANUP: &str = r#"rm -f -- "$1" "$1.sum" && echo 'Removed the temporary archive from the server.'
"#;

pub const EXIT_CHECKSUM: u32 = 20;
pub const EXIT_EXISTS: u32 = 21;
pub const EXIT_UNSAFE: u32 = 22;

/// The destination server: an unpacker, a writable place for the folder, free space, and
/// (when the name is known) what happens to an existing folder of that name.
/// Args: parent, name or "-", existing policy, needed KB or "-".
const CHECK_DEST: &str = r#"name=$2; policy=$3; need=$4
if command -v unzip >/dev/null 2>&1; then tool=unzip
elif command -v python3 >/dev/null 2>&1; then tool='python3 (unzip is not installed)'
else echo 'Neither unzip nor python3 is installed on the server. Install unzip, for example: sudo apt install unzip' >&2; exit 11; fi
d=$p; while [ ! -d "$d" ]; do d=$(dirname -- "$d"); done
if [ ! -w "$d" ]; then echo "No permission to write in $d" >&2; exit 13; fi
[ -d "$p" ] || echo "Will create:   $p"
free=$(df -Pk "$d" | awk 'NR==2 {print $4}')
tmpfree=$(df -Pk /tmp | awk 'NR==2 {print $4}')
echo "Destination:   $p"
echo "Unzip with:    $tool"
echo "Free there:    $(( ${free:-0} / 1024 )) MB, in /tmp: $(( ${tmpfree:-0} / 1024 )) MB"
if [ "$name" != - ] && [ -e "$p/$name" ]; then
  case $policy in
    stop) echo "$p/$name already exists." >&2; exit 21 ;;
    backup) echo "$p/$name exists; it will be moved to $name.bak-<date> first." ;;
    *) echo "$p/$name exists; the copy will be unpacked over it." ;;
  esac
fi
if [ "$need" != - ] && [ -n "$tmpfree" ] && [ "$tmpfree" -lt "$need" ]; then
  echo "Not enough free space in /tmp for the archive." >&2; exit 12
fi
"#;

/// Verifies the uploaded archive, deals with an existing folder, unpacks.
/// Args: parent, archive, name, existing policy, expected SHA-256 or "-", unpacker script.
const UNPACK_REMOTE: &str = r#"a=$2; name=$3; policy=$4; want=$5
if [ "$want" != - ]; then
  if command -v sha256sum >/dev/null 2>&1; then sum=$(sha256sum -- "$a" | cut -d' ' -f1)
  elif command -v shasum >/dev/null 2>&1; then sum=$(shasum -a 256 "$a" | cut -d' ' -f1)
  else sum=; echo 'No SHA-256 tool on the server; only the upload itself was checked.'; fi
  if [ -n "$sum" ] && [ "$sum" != "$want" ]; then echo 'The uploaded archive does not match: it was damaged in transit.' >&2; exit 20; fi
  [ -n "$sum" ] && echo 'Checksum OK'
fi
mkdir -p -- "$p" && cd -- "$p" || exit 13
if [ -e "$name" ] || [ -L "$name" ]; then
  case $policy in
    stop) echo "$p/$name already exists." >&2; exit 21 ;;
    backup) b="$name.bak-$(date +%Y%m%d-%H%M%S)"; mv -- "$name" "$b" || exit 13; echo "Moved the existing $name to $b" ;;
    *) echo "Unpacking over the existing $name" ;;
  esac
fi
if command -v unzip >/dev/null 2>&1; then unzip -o -q "$a" -d .; rc=$?
else python3 -c "$6" "$a" .; rc=$?; fi
[ $rc -eq 0 ] || exit $rc
echo "Unpacked into $p/$name"
"#;

/// Args: parent, name, owner.
const OWNER: &str = r#"chown -R -- "$3" "$p/$2" && echo "Owner of $p/$2 is now $3"
"#;

const PYZIP: &str = include_str!("pyzip.py");
const PYUNZIP: &str = include_str!("pyunzip.py");

pub fn check_dest_command(
    parent: &str,
    name: Option<&str>,
    existing: Existing,
    need_kb: Option<u64>,
) -> AppResult<String> {
    let parent = check_remote_dir(parent)?;
    let need = need_kb.map(|k| k.to_string());
    Ok(remote(
        &format!("{RESOLVE}{CHECK_DEST}"),
        &[
            parent,
            name.unwrap_or("-"),
            existing.as_arg(),
            need.as_deref().unwrap_or("-"),
        ],
    ))
}

pub fn unpack_remote_command(
    parent: &str,
    archive: &str,
    name: &str,
    existing: Existing,
    sha256: Option<&str>,
) -> AppResult<String> {
    let parent = check_remote_dir(parent)?;
    check_entry_name(name)?;
    if !is_own_archive(archive) {
        return Err(AppError::Validation(format!(
            "unexpected archive {archive}"
        )));
    }
    Ok(remote(
        &format!("{RESOLVE}{UNPACK_REMOTE}"),
        &[
            parent,
            archive,
            name,
            existing.as_arg(),
            sha256.unwrap_or("-"),
            PYUNZIP,
        ],
    ))
}

pub fn owner_command(parent: &str, name: &str, owner: &str) -> AppResult<String> {
    let parent = check_remote_dir(parent)?;
    check_entry_name(name)?;
    check_owner(owner)?;
    Ok(remote(&format!("{RESOLVE}{OWNER}"), &[parent, name, owner]))
}

/// A single path component (the folder's name inside the archive).
fn check_entry_name(name: &str) -> AppResult<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        return Err(AppError::Validation(format!(
            "unexpected folder name {name:?}"
        )));
    }
    Ok(())
}

/// `sh -c '<script>' ridgessh "$HOME" '<arg>'…`: the login shell expands `$HOME` (so it is the
/// login user's even when the command runs under `sudo`); the script sees it as `$h` and its
/// own arguments as `$1`…
fn remote(script: &str, args: &[&str]) -> String {
    let args: String = args
        .iter()
        .map(|a| format!(" {}", posix_quote(a)))
        .collect();
    let script = format!("h=$1; shift\n{script}");
    format!("sh -c {} ridgessh \"$HOME\"{args}", posix_quote(&script))
}

/// Runs `command` (one of the commands built here) as root. `-p` gives a clear prompt; the
/// ssh session needs a terminal (`-t`) for it.
pub fn with_sudo(command: &str) -> String {
    format!(
        "sudo -p {} {command}",
        posix_quote("[sudo] password for %u on %h: ")
    )
}

/// Checks the folder, the packer and free space in /tmp.
pub fn check_command(path: &str) -> AppResult<String> {
    let path = check_remote_path(path)?;
    Ok(remote(&format!("{RESOLVE}{CHECK}"), &[path]))
}

/// Zips the folder into `archive` and writes `archive.sum` ("<sha256> <bytes> <name>").
pub fn pack_command(path: &str, archive: &str, excludes: &[String]) -> AppResult<String> {
    let path = check_remote_path(path)?;
    let mut args = vec![path, archive, PYZIP];
    args.extend(excludes.iter().map(String::as_str));
    Ok(remote(&format!("{RESOLVE}{PACK}"), &args))
}

/// What the user typed to leave out: one name or relative path per entry, wildcards allowed.
pub fn check_excludes(excludes: &[String]) -> AppResult<()> {
    if excludes.len() > 100 {
        return Err(AppError::Validation("at most 100 exclude patterns".into()));
    }
    for e in excludes {
        let t = e.trim();
        if t.is_empty()
            || t.len() > 256
            || t.chars().any(char::is_control)
            || t.starts_with('-')
            || t.split('/').any(|c| c == "..")
        {
            return Err(AppError::Validation(format!(
                "unusable exclude pattern {e:?}"
            )));
        }
        glob::Pattern::new(t)
            .map_err(|err| AppError::Validation(format!("exclude pattern {e:?}: {err}")))?;
    }
    Ok(())
}

/// Turns the user's patterns into archive-path patterns, the same for `zip -x`, the python
/// packer and the local packer (`*` matches across `/` in all three). A plain name
/// (`node_modules`, `*.log`) matches at any depth; one with a `/` (`storage/cache`) is
/// relative to the folder, whose name is `name` (`*` when not known in advance).
pub fn exclude_patterns(name: Option<&str>, excludes: &[String]) -> Vec<String> {
    excludes
        .iter()
        .map(|e| e.trim().trim_matches('/'))
        .filter(|e| !e.is_empty())
        .flat_map(|e| {
            let base = if e.contains('/') {
                format!("{}/{e}", name.unwrap_or("*"))
            } else {
                format!("*/{e}")
            };
            [base.clone(), format!("{base}/*")]
        })
        .collect()
}

/// Prints the archive's `.sum` file (see [`PACK`]).
pub fn read_sum_command(archive: &str) -> AppResult<String> {
    own_archive(archive)?;
    Ok(remote(r#"cat -- "$1.sum""#, &[archive]))
}

/// Writes the archive to stdout, for streaming.
pub fn send_command(archive: &str) -> AppResult<String> {
    own_archive(archive)?;
    Ok(remote(r#"exec cat -- "$1""#, &[archive]))
}

/// Saves stdin as the (private) inbox archive, for streaming.
pub fn receive_command(inbox: &str) -> AppResult<String> {
    own_archive(inbox)?;
    Ok(remote(
        r#"umask 077; rm -f -- "$1" && exec cat > "$1""#,
        &[inbox],
    ))
}

fn own_archive(archive: &str) -> AppResult<()> {
    if is_own_archive(archive) {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "unexpected archive {archive}"
        )))
    }
}

pub fn cleanup_command(archive: &str) -> AppResult<String> {
    if !is_own_archive(archive) {
        return Err(AppError::Validation(format!(
            "refusing to delete {archive}"
        )));
    }
    Ok(remote(CLEANUP, &[archive]))
}

/// The `.sum` file written next to the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Lowercase hex SHA-256, or `None` when the server had no tool to compute it.
    pub sha256: Option<String>,
    pub bytes: u64,
    /// The folder's name (the archive's top-level entry).
    pub name: String,
}

pub fn parse_manifest(text: &str) -> AppResult<Manifest> {
    let bad = || AppError::Other(format!("unexpected archive summary: {text:?}"));
    let line = text.trim_end_matches(['\r', '\n']);
    let mut parts = line.splitn(3, ' ');
    let (sum, bytes, name) = (
        parts.next().ok_or_else(bad)?,
        parts.next().ok_or_else(bad)?,
        parts.next().ok_or_else(bad)?,
    );
    let sha256 = match sum {
        "-" => None,
        s if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Some(s.to_ascii_lowercase())
        }
        _ => return Err(bad()),
    };
    // The name becomes a local file name: refuse anything that is not one plain component.
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':'])
        || name.chars().any(char::is_control)
    {
        return Err(bad());
    }
    Ok(Manifest {
        sha256,
        bytes: bytes.parse().map_err(|_| bad())?,
        name: name.to_string(),
    })
}

/// `dir/name.zip`, or `dir/name (2).zip`… when that exists.
pub fn free_name(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut n = 1;
    loop {
        let file = if n == 1 {
            format!("{stem}{ext}")
        } else {
            format!("{stem} ({n}){ext}")
        };
        let p = dir.join(file);
        if !p.exists() {
            return p;
        }
        n += 1;
    }
}

/// Where an existing folder is moved before unpacking over it: `name.bak-YYYYMMDD-HHMMSS`.
pub fn backup_name(dir: &Path, name: &str) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    free_name(dir, &format!("{name}.bak-{stamp}"), "")
}

/// SHA-256 of a file, lowercase hex.
pub fn sha256_file(path: &Path) -> AppResult<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Free bytes on the disk holding `dir`, when the OS tells.
pub fn free_space(dir: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let wide: Vec<u16> = dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut free: u64 = 0;
        // SAFETY: NUL-terminated path; the out pointers are valid or null as documented.
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(free)
    }
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let c = CString::new(dir.as_os_str().as_bytes()).ok()?;
        // SAFETY: zeroed statvfs is a valid out value; the path is NUL-terminated.
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        let ok = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
        if ok != 0 {
            return None;
        }
        #[allow(clippy::unnecessary_cast)] // the field types differ between platforms
        let bytes = st.f_bavail as u64 * st.f_frsize as u64;
        Some(bytes)
    }
}

/// The system `tar` (bsdtar reads zip on Windows 10+ and macOS).
pub fn find_tar() -> AppResult<PathBuf> {
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        let p = PathBuf::from(root).join("System32").join("tar.exe");
        if p.is_file() {
            return Ok(p);
        }
    }
    which::which("tar").map_err(|_| {
        AppError::NotFound("tar was not found, so the archive cannot be unpacked here".into())
    })
}

#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod phase3_tests;
#[cfg(test)]
mod relay_tests;
#[cfg(test)]
mod tests;
