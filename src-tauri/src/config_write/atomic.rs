//! Crash-safe file replacement that keeps the original file's permissions.
//!
//! The new content goes to a temp file in the same directory, is flushed to disk, and then
//! swapped in. Readers see either the old or the new file, never a partial one.
//! - Windows: `ReplaceFileW` keeps the replaced file's ACL, attributes and creation time.
//!   OpenSSH for Windows refuses a config that other users can write, so the ACL matters.
//! - Unix: the original mode is copied (new files get `0600`), then `rename`.
//!
//! A symlinked target (dotfile managers) is resolved first, so the link stays a link and the
//! file it points to gets the new content.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub fn replace_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let resolved;
    let path = if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        resolved = fs::canonicalize(path)?;
        resolved.as_path()
    } else {
        path
    };
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp = temp_path(path, dir);
    let result = (|| {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()?;
        drop(f);
        if path.exists() {
            swap_in(path, &tmp)
        } else {
            set_new_file_permissions(&tmp)?;
            fs::rename(&tmp, path)
        }?;
        sync_dir(dir);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn temp_path(path: &Path, dir: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    dir.join(format!(
        ".{name}.ridgessh-{}.tmp",
        uuid::Uuid::new_v4().simple()
    ))
}

#[cfg(windows)]
fn swap_in(target: &Path, replacement: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_WRITE_THROUGH, ReplaceFileW};

    let wide = |p: &Path| -> Vec<u16> {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let (target, replacement) = (wide(target), wide(replacement));
    // SAFETY: both strings are NUL-terminated and outlive the call; the optional backup,
    // exclude and reserved arguments are null as documented.
    let ok = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            replacement.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_WRITE_THROUGH,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn swap_in(target: &Path, replacement: &Path) -> io::Result<()> {
    let perms = fs::metadata(target)?.permissions();
    fs::set_permissions(replacement, perms)?;
    fs::rename(replacement, target)
}

/// Makes the rename itself durable (Unix keeps it in the directory entry).
#[cfg(unix)]
fn sync_dir(dir: &Path) {
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) {}

#[cfg(unix)]
fn set_new_file_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

/// On Windows a new file inherits the ACL of `~/.ssh`, which is what OpenSSH expects.
#[cfg(not(unix))]
fn set_new_file_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_and_creates() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config");
        replace_file(&p, b"one").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"one");
        replace_file(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        let leftovers: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "temp file left behind");
    }

    #[cfg(unix)]
    #[test]
    fn keeps_mode_and_symlinks() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("dotfiles-config");
        fs::write(&real, b"old").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).unwrap();
        let link = dir.path().join("config");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        replace_file(&link, b"new").unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&real).unwrap(), b"new");
        let mode = fs::metadata(&real).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);

        let fresh = dir.path().join("fresh");
        replace_file(&fresh, b"x").unwrap();
        let mode = fs::metadata(&fresh).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    /// The custom ACL on the original file must survive the replace.
    #[cfg(windows)]
    #[test]
    fn keeps_windows_acl() {
        use std::process::Command;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config");
        fs::write(&p, b"old").unwrap();
        let user = std::env::var("USERNAME").unwrap();
        // Like a hardened ~/.ssh/config: no inheritance, only the current user.
        let status = Command::new("icacls")
            .arg(&p)
            .args(["/inheritance:r", "/grant:r", &format!("{user}:F")])
            .output()
            .unwrap();
        assert!(status.status.success(), "{status:?}");
        let acl = |p: &Path| {
            let out = Command::new("icacls").arg(p).output().unwrap();
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        let before = acl(&p);
        // "(I)" marks inherited entries: a plain rename would bring those back.
        assert!(!before.contains("(I)"), "{before}");
        replace_file(&p, b"new").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"new");
        assert_eq!(acl(&p), before);
    }
}
