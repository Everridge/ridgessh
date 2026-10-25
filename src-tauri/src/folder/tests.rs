use std::path::Path;
use std::process::Command;

use super::*;

#[test]
fn remote_paths() {
    for ok in [
        "/var/www/site",
        "~/app",
        "~",
        "relative/dir",
        "/srv/my app/",
    ] {
        assert!(check_remote_path(ok).is_ok(), "{ok}");
    }
    for bad in ["", "  ", "/", "//", "a/..", ".", "x\ny"] {
        assert!(check_remote_path(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn manifests() {
    let sum = "a".repeat(64);
    assert_eq!(
        parse_manifest(&format!("{sum} 1234 my app\n")).unwrap(),
        Manifest {
            sha256: Some(sum.clone()),
            bytes: 1234,
            name: "my app".into()
        }
    );
    assert_eq!(parse_manifest("- 5 site\r\n").unwrap().sha256, None);
    for bad in [
        format!("{sum} 12 ../etc"),
        format!("{sum} 12 a/b"),
        format!("{sum} 12 C:x"),
        format!("{sum} x site"),
        "short 1 site".to_string(),
        format!("{sum} 12"),
    ] {
        assert!(parse_manifest(&bad).is_err(), "{bad}");
    }
}

#[test]
fn cleanup_only_touches_own_archives() {
    assert!(cleanup_command(&remote_archive("0123456789ab")).is_ok());
    for other in [
        "/tmp/ridgessh-x.zip",
        "/etc/passwd",
        "/tmp/ridgessh-0123456789ab.zip.x",
    ] {
        assert!(cleanup_command(other).is_err(), "{other}");
    }
}

#[test]
fn free_names() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        free_name(dir.path(), "site", ".zip"),
        dir.path().join("site.zip")
    );
    std::fs::write(dir.path().join("site.zip"), b"").unwrap();
    std::fs::write(dir.path().join("site (2).zip"), b"").unwrap();
    assert_eq!(
        free_name(dir.path(), "site", ".zip"),
        dir.path().join("site (3).zip")
    );
    let b = backup_name(dir.path(), "site");
    assert!(
        b.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("site.bak-")
    );
}

fn sh() -> Option<std::path::PathBuf> {
    which::which("sh").ok()
}

/// Runs a generated remote command line the way the login shell on the server would.
fn run_remote(sh: &Path, cmd: &str, cwd: &Path, home: &Path) -> std::process::Output {
    Command::new(sh)
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .env("HOME", home)
        .output()
        .unwrap()
}

/// A folder with a space and a quote in its name, a subfolder and an executable file.
fn sample_tree(root: &Path) -> std::path::PathBuf {
    let dir = root.join("my 'app'");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("a.txt"), b"hello").unwrap();
    std::fs::write(dir.join("sub").join("b.txt"), b"world").unwrap();
    std::fs::write(dir.join("run.sh"), b"#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.join("run.sh"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        std::os::unix::fs::symlink("a.txt", dir.join("link")).unwrap();
    }
    dir
}

fn entries(zip: &Path) -> Vec<String> {
    let mut a = ::zip::ZipArchive::new(std::fs::File::open(zip).unwrap()).unwrap();
    let mut names: Vec<String> = (0..a.len())
        .map(|i| {
            a.by_index(i)
                .unwrap()
                .name()
                .unwrap()
                .trim_start_matches("./")
                .to_string()
        })
        .collect();
    names.sort();
    names
}

fn python_works() -> bool {
    Command::new("python3")
        .args(["-c", "print(1)"])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn check_script_reports_problems_by_exit_code() {
    let Some(sh) = sh() else { return };
    let home = tempfile::tempdir().unwrap();
    sample_tree(home.path());
    if !(which::which("zip").is_ok() || python_works()) {
        eprintln!("neither zip nor python3; skipping");
        return;
    }
    let out = run_remote(
        &sh,
        &check_command("~/my 'app'").unwrap(),
        home.path(),
        home.path(),
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("Folder:") && text.contains("my 'app'"),
        "{text}"
    );

    let missing = run_remote(
        &sh,
        &check_command("~/nope").unwrap(),
        home.path(),
        home.path(),
    );
    assert_eq!(missing.status.code(), Some(EXIT_NOT_FOUND as i32));
}

#[test]
fn pack_script_zips_relative_to_the_parent_and_writes_a_summary() {
    let Some(sh) = sh() else { return };
    if !(which::which("zip").is_ok() || python_works()) {
        eprintln!("neither zip nor python3; skipping");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let dir = sample_tree(home.path());
    let out_dir = tempfile::tempdir().unwrap();
    let archive = out_dir.path().join("out.zip");
    // The archive path as the remote shell sees it (forward slashes work everywhere).
    let archive_arg = archive.display().to_string().replace('\\', "/");
    let out = run_remote(
        &sh,
        &pack_command(
            &dir.display().to_string().replace('\\', "/"),
            &archive_arg,
            &[],
        )
        .unwrap(),
        home.path(),
        home.path(),
    );
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let names = entries(&archive);
    for want in ["my 'app'/a.txt", "my 'app'/sub/b.txt", "my 'app'/run.sh"] {
        assert!(names.iter().any(|n| n == want), "{want} not in {names:?}");
    }
    let sum = std::fs::read_to_string(out_dir.path().join("out.zip.sum")).unwrap();
    let m = parse_manifest(&sum).unwrap();
    assert_eq!(m.name, "my 'app'");
    assert_eq!(m.bytes, std::fs::metadata(&archive).unwrap().len());
    if let Some(s) = m.sha256 {
        assert_eq!(s, sha256_file(&archive).unwrap());
    }

    // And the system tar (bsdtar on Windows/macOS) unpacks it here.
    let Ok(tar) = find_tar() else { return };
    let dest = tempfile::tempdir().unwrap();
    let st = Command::new(tar)
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(dest.path())
        .output()
        .unwrap();
    if !st.status.success() {
        // GNU tar (some Linux machines) does not read zip; bsdtar does.
        eprintln!(
            "tar cannot read zip here: {}",
            String::from_utf8_lossy(&st.stderr)
        );
        return;
    }
    let got = std::fs::read(dest.path().join("my 'app'").join("sub").join("b.txt")).unwrap();
    assert_eq!(got, b"world");
}

/// The python fallback keeps permissions and symlinks like `zip -r -y`.
#[test]
fn python_fallback_packer() {
    if !python_works() {
        eprintln!("no python3; skipping");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    sample_tree(root.path());
    let archive = root.path().join("py.zip");
    let out = Command::new("python3")
        .args(["-c", PYZIP, "my 'app'"])
        .arg(&archive)
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let names = entries(&archive);
    assert!(names.contains(&"my 'app'/".to_string()), "{names:?}");
    assert!(
        names.contains(&"my 'app'/sub/b.txt".to_string()),
        "{names:?}"
    );
    #[cfg(unix)]
    {
        let mut a = ::zip::ZipArchive::new(std::fs::File::open(&archive).unwrap()).unwrap();
        let mode = |a: &mut ::zip::ZipArchive<std::fs::File>, n: &str| {
            a.by_name(n).unwrap().unix_mode().unwrap()
        };
        assert_eq!(mode(&mut a, "my 'app'/run.sh") & 0o777, 0o755);
        assert_eq!(
            mode(&mut a, "my 'app'/link") & 0o170000,
            0o120000,
            "stored as a link"
        );
    }
}
