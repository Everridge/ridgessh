//! Folder transfers (M11): a remote folder is zipped on the server, downloaded with a
//! checksum check, optionally unpacked locally, and the server's temporary files are removed.
//!
//! Each step is one ssh/scp process in the tab's PTY (see [`runner`]), so password and
//! host-key prompts work. Remote steps are fixed `sh -c` scripts; everything that varies (the
//! folder path, archive names) is passed as quoted positional arguments, never spliced into
//! the script. Steps report success through their exit codes, and the archive's checksum
//! travels in a small `.sum` file next to it, so nothing depends on parsing terminal output.

pub mod runner;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::terminal::ssh_args::posix_quote;

/// What to transfer and what to do with it. Phase 1: a remote folder to this PC.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSpec {
    pub host_id: String,
    /// Connect as this user (`user@alias`, see `target_args`).
    #[serde(default)]
    pub user: Option<String>,
    /// The remote folder: absolute, `~/…`, or relative to the login directory.
    pub remote_path: String,
    /// Local folder that receives `<name>.zip` (and `<name>/` when unpacking).
    pub local_dir: String,
    /// Unpack the archive here after downloading.
    #[serde(default)]
    pub unpack: bool,
    /// Keep the `.zip` after unpacking (always kept when not unpacking).
    #[serde(default = "yes")]
    pub keep_zip: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StepKind {
    Check,
    Pack,
    Download,
    Unpack,
    Cleanup,
}

impl StepKind {
    pub fn label(self) -> &'static str {
        match self {
            StepKind::Check => "Check",
            StepKind::Pack => "Zip on server",
            StepKind::Download => "Download",
            StepKind::Unpack => "Unpack locally",
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
    let mut out = vec![StepKind::Check, StepKind::Pack, StepKind::Download];
    if spec.unpack {
        out.push(StepKind::Unpack);
    }
    out.push(StepKind::Cleanup);
    out
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

fn is_own_archive(path: &str) -> bool {
    path.strip_prefix("/tmp/ridgessh-")
        .and_then(|r| r.strip_suffix(".zip"))
        .is_some_and(|id| id.len() == 12 && id.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Resolves `~` and `~/…` like a shell would (quoting stops the shell from doing it).
const RESOLVE: &str = r#"p=$1
case $p in '~') p=$HOME ;; '~/'*) p=$HOME/${p#'~/'} ;; esac
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

const PACK: &str = r#"out=$2
cd -- "$(dirname -- "$p")" || exit 13
name=$(basename -- "$p")
umask 077
rm -f -- "$out" "$out.sum"
if command -v zip >/dev/null 2>&1; then zip -r -y -q "$out" "./$name"; rc=$?
else python3 -c "$3" "$name" "$out"; rc=$?; fi
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
echo "Zipped $name: $(( bytes / 1048576 )) MB"
exit $rc
"#;

const CLEANUP: &str = r#"rm -f -- "$1" "$1.sum" && echo 'Removed the temporary archive from the server.'
"#;

const PYZIP: &str = include_str!("pyzip.py");

fn remote(script: &str, args: &[&str]) -> String {
    let args: String = args
        .iter()
        .map(|a| format!(" {}", posix_quote(a)))
        .collect();
    format!("sh -c {} ridgessh{args}", posix_quote(script))
}

/// Checks the folder, the packer and free space in /tmp.
pub fn check_command(path: &str) -> AppResult<String> {
    let path = check_remote_path(path)?;
    Ok(remote(&format!("{RESOLVE}{CHECK}"), &[path]))
}

/// Zips the folder into `archive` and writes `archive.sum` ("<sha256> <bytes> <name>").
pub fn pack_command(path: &str, archive: &str) -> AppResult<String> {
    let path = check_remote_path(path)?;
    Ok(remote(&format!("{RESOLVE}{PACK}"), &[path, archive, PYZIP]))
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
mod tests;
