//! Phase 2: steps per direction, validation, zipping on this PC, and the destination-side
//! scripts run with a local `sh` (and the stand-in runner end to end on Unix).

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

fn remote_end(path: &str) -> Endpoint {
    Endpoint::Remote {
        host_id: "h".into(),
        user: None,
        path: path.into(),
    }
}

fn local_end(path: &Path) -> Endpoint {
    Endpoint::Local {
        path: path.display().to_string(),
    }
}

fn spec(source: Endpoint, destination: Endpoint) -> FolderSpec {
    FolderSpec {
        source,
        destination,
        unpack: false,
        keep_zip: true,
        keep_copy: None,
        existing: Existing::Backup,
        owner: None,
        excludes: vec![],
        sudo_source: false,
        sudo_dest: false,
        before: None,
        after: None,
    }
}

#[test]
fn steps_per_direction() {
    use StepKind::*;
    let tmp = tempfile::tempdir().unwrap();
    let mut down = spec(remote_end("/srv/app"), local_end(tmp.path()));
    assert_eq!(steps(&down), [Check, Pack, Download, Cleanup]);
    down.unpack = true;
    assert_eq!(steps(&down), [Check, Pack, Download, Unpack, Cleanup]);

    let mut up = spec(local_end(tmp.path()), remote_end("/srv"));
    assert_eq!(
        steps(&up),
        [Check, CheckDest, PackLocal, Upload, UnpackRemote, Cleanup]
    );
    up.owner = Some("www-data:www-data".into());
    assert_eq!(
        steps(&up),
        [
            Check,
            CheckDest,
            PackLocal,
            Upload,
            UnpackRemote,
            SetOwner,
            Cleanup
        ]
    );

    let copy = spec(remote_end("/srv/app"), remote_end("/srv"));
    assert_eq!(
        steps(&copy),
        [
            Check,
            CheckDest,
            Pack,
            Download,
            Upload,
            UnpackRemote,
            Cleanup
        ]
    );
}

#[test]
fn spec_validation() {
    let tmp = tempfile::tempdir().unwrap();
    let ok = spec(local_end(tmp.path()), remote_end("~/sites"));
    assert!(check_spec(&ok).is_ok());
    let both_local = spec(local_end(tmp.path()), local_end(tmp.path()));
    assert!(check_spec(&both_local).is_err());
    let missing = spec(local_end(&tmp.path().join("nope")), remote_end("/srv"));
    assert!(check_spec(&missing).is_err());
    let relative = spec(
        remote_end("/srv/app"),
        Endpoint::Local { path: "dl".into() },
    );
    assert!(check_spec(&relative).is_err());
    let mut bad_owner = ok.clone();
    bad_owner.owner = Some("root; rm -rf /".into());
    assert!(check_spec(&bad_owner).is_err());

    for good in ["www-data", "www-data:www-data", "1000:1000", "deploy.bot"] {
        assert!(check_owner(good).is_ok(), "{good}");
    }
    for bad in ["", "-R", "a:b:c", "a b", "a:", "$(id)"] {
        assert!(check_owner(bad).is_err(), "{bad}");
    }
    assert_eq!(
        source_name(&ok).as_deref(),
        tmp.path().file_name().and_then(|n| n.to_str())
    );
    assert_eq!(
        source_name(&spec(remote_end("~/app/"), remote_end("/x"))).as_deref(),
        Some("app")
    );
    assert_eq!(source_name(&spec(remote_end("~"), remote_end("/x"))), None);
}

#[test]
fn inbox_archives_are_cleanable() {
    assert!(cleanup_command(&remote_inbox("0123456789ab")).is_ok());
    assert!(cleanup_command("/tmp/ridgessh-0123456789ab-out.zip").is_err());
}

/// A folder with a nested folder, an empty folder and a file with a space in its name.
fn tree(root: &Path) -> PathBuf {
    let dir = root.join("site");
    std::fs::create_dir_all(dir.join("assets").join("img")).unwrap();
    std::fs::create_dir_all(dir.join("empty")).unwrap();
    std::fs::write(dir.join("index.html"), b"<h1>hi</h1>").unwrap();
    std::fs::write(dir.join("assets").join("my file.css"), b"body{}").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(dir.join("deploy.sh"), b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(
            dir.join("deploy.sh"),
            std::fs::Permissions::from_mode(0o750),
        )
        .unwrap();
        std::os::unix::fs::symlink("index.html", dir.join("home.html")).unwrap();
    }
    dir
}

fn zip_names(zip: &Path) -> Vec<String> {
    let mut a = ::zip::ZipArchive::new(std::fs::File::open(zip).unwrap()).unwrap();
    let mut names: Vec<String> = (0..a.len())
        .map(|i| a.by_index(i).unwrap().name().unwrap().to_string())
        .collect();
    names.sort();
    names
}

#[test]
fn local_pack_layout() {
    let root = tempfile::tempdir().unwrap();
    let dir = tree(root.path());
    let out = root.path().join("out.zip");
    let report = local::pack(&dir, &out, &[]).unwrap();
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let names = zip_names(&out);
    for want in [
        "site/",
        "site/assets/",
        "site/assets/img/",
        "site/assets/my file.css",
        "site/empty/",
        "site/index.html",
    ] {
        assert!(names.iter().any(|n| n == want), "{want} not in {names:?}");
    }
    #[cfg(unix)]
    {
        let mut a = ::zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        assert_eq!(
            a.by_name("site/deploy.sh").unwrap().unix_mode().unwrap() & 0o777,
            0o750
        );
        assert_eq!(
            a.by_name("site/home.html").unwrap().unix_mode().unwrap() & 0o170000,
            0o120000
        );
    }
    let (files, bytes) = local::folder_size(&dir);
    assert_eq!(files, report.files);
    assert_eq!(bytes, report.bytes);
    // Same folder, same archive.
    let again = root.path().join("again.zip");
    local::pack(&dir, &again, &[]).unwrap();
    assert_eq!(sha256_file(&out).unwrap(), sha256_file(&again).unwrap());
}

fn sh() -> Option<PathBuf> {
    which::which("sh").ok()
}

/// A path as the shell sees it (forward slashes work in Git's sh on Windows too).
fn sh_path(p: &Path) -> String {
    p.display().to_string().replace('\\', "/")
}

fn run(sh: &Path, cmd: &str) -> std::process::Output {
    Command::new(sh).arg("-c").arg(cmd).output().unwrap()
}

/// The unpack script, without the archive-name check (tests use their own temp paths).
fn unpack_cmd(parent: &Path, archive: &Path, name: &str, existing: Existing, sha: &str) -> String {
    remote(
        &format!("{RESOLVE}{UNPACK_REMOTE}"),
        &[
            &sh_path(parent),
            &sh_path(archive),
            name,
            existing.as_arg(),
            sha,
            PYUNZIP,
        ],
    )
}

#[test]
fn unpack_script_verifies_and_handles_an_existing_folder() {
    let Some(sh) = sh() else { return };
    let have_unzip = which::which("unzip").is_ok();
    if !have_unzip && Command::new("python3").arg("-V").output().is_err() {
        eprintln!("no unzip or python3; skipping");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let src = tree(root.path());
    let zip = root.path().join("site.zip");
    local::pack(&src, &zip, &[]).unwrap();
    let sha = sha256_file(&zip).unwrap();
    let dest = root.path().join("dest").join("new parent");

    // Fresh: the parent is created and the folder unpacked.
    let out = run(
        &sh,
        &unpack_cmd(&dest, &zip, "site", Existing::Backup, &sha),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read(dest.join("site").join("assets").join("my file.css")).unwrap(),
        b"body{}"
    );
    assert!(dest.join("site").join("empty").is_dir());

    // A wrong checksum changes nothing.
    std::fs::write(dest.join("site").join("marker"), b"old").unwrap();
    let bad = run(
        &sh,
        &unpack_cmd(&dest, &zip, "site", Existing::Backup, &"0".repeat(64)),
    );
    assert_eq!(bad.status.code(), Some(EXIT_CHECKSUM as i32));
    assert!(dest.join("site").join("marker").exists());

    // Stop: refuses, nothing moved.
    let stop = run(&sh, &unpack_cmd(&dest, &zip, "site", Existing::Stop, &sha));
    assert_eq!(stop.status.code(), Some(EXIT_EXISTS as i32));
    assert!(dest.join("site").join("marker").exists());

    // Merge: unpacked over it, the extra file stays.
    let merge = run(&sh, &unpack_cmd(&dest, &zip, "site", Existing::Merge, &sha));
    assert!(
        merge.status.success(),
        "{}",
        String::from_utf8_lossy(&merge.stderr)
    );
    assert!(dest.join("site").join("marker").exists());

    // Backup: the old folder is moved aside first.
    let backup = run(
        &sh,
        &unpack_cmd(&dest, &zip, "site", Existing::Backup, &sha),
    );
    assert!(
        backup.status.success(),
        "{}",
        String::from_utf8_lossy(&backup.stderr)
    );
    assert!(!dest.join("site").join("marker").exists());
    let backups: Vec<_> = std::fs::read_dir(&dest)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("site.bak-"))
        .collect();
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert!(dest.join(&backups[0]).join("marker").exists());
}

#[test]
fn check_dest_script_stops_early_on_an_existing_folder() {
    let Some(sh) = sh() else { return };
    if which::which("unzip").is_err() && Command::new("python3").arg("-V").output().is_err() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("site")).unwrap();
    let parent = sh_path(root.path());
    let cmd = |existing, name: Option<&str>| {
        check_dest_command(&parent, name, existing, Some(1)).unwrap()
    };
    let out = run(&sh, &cmd(Existing::Stop, Some("site")));
    assert_eq!(out.status.code(), Some(EXIT_EXISTS as i32));
    let out = run(&sh, &cmd(Existing::Backup, Some("site")));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("will be moved"));
    // A parent that does not exist yet is fine when an ancestor is writable.
    let deeper =
        check_dest_command(&format!("{parent}/a/b"), Some("site"), Existing::Stop, None).unwrap();
    let out = run(&sh, &deeper);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("Will create"));
}

/// The python unpacker restores modes and links, and refuses paths outside the folder.
#[test]
fn python_fallback_unpacker() {
    if !Command::new("python3")
        .args(["-c", "print(1)"])
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("no python3; skipping");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let src = tree(root.path());
    let zip = root.path().join("site.zip");
    local::pack(&src, &zip, &[]).unwrap();
    let dest = root.path().join("out");
    std::fs::create_dir(&dest).unwrap();
    let out = Command::new("python3")
        .args(["-c", PYUNZIP])
        .arg(&zip)
        .arg(&dest)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read(dest.join("site").join("assets").join("my file.css")).unwrap(),
        b"body{}"
    );
    assert!(dest.join("site").join("empty").is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dest.join("site").join("deploy.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o750);
        assert_eq!(
            std::fs::read_link(dest.join("site").join("home.html")).unwrap(),
            Path::new("index.html")
        );
    }

    // An archive with "../" is refused before anything is written.
    let evil = root.path().join("evil.zip");
    {
        let mut w = ::zip::ZipWriter::new(std::fs::File::create(&evil).unwrap());
        w.start_file("../escaped.txt", ::zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut w, b"x").unwrap();
        w.finish().unwrap();
    }
    let out = Command::new("python3")
        .args(["-c", PYUNZIP])
        .arg(&evil)
        .arg(&dest)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(EXIT_UNSAFE as i32));
    assert!(!root.path().join("escaped.txt").exists());
}

/// The whole runner against stand-ins: `ssh` runs the remote command with the local shell
/// and `scp` copies, so every step runs for real (in PTYs), just without a network. Covers
/// server to PC, PC to server, and server to server.
#[cfg(unix)]
mod end_to_end {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Mutex};

    use tauri::ipc::{Channel, InvokeResponseBody};

    use super::*;
    use crate::folder::runner::{Conn, RunState, Runner, StepEvent, StepStatus};
    use crate::terminal::TerminalManager;

    struct Tools {
        _dir: tempfile::TempDir,
        ssh: PathBuf,
        scp: PathBuf,
    }

    fn tools() -> Tools {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, body: &str| {
            let p = dir.path().join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        };
        // ssh [-t] <destination> <remote command>: runs the last argument.
        let ssh = write("ssh", r#"for a; do last=$a; done; exec /bin/sh -c "$last""#);
        // scp [-q] <from> <to>, either side possibly "host:/path"
        let scp = write(
            "scp",
            r#"[ "$1" = -q ] && shift; exec cp "${1#*:}" "${2#*:}""#,
        );
        Tools {
            _dir: dir,
            ssh,
            scp,
        }
    }

    fn conn(name: &str) -> Conn {
        Conn {
            options: vec![],
            destination: name.into(),
        }
    }

    /// Runs `spec`; returns the exit code, the tab's text and the final step statuses.
    fn execute(spec: FolderSpec) -> (Option<u32>, String, Vec<StepStatus>, RunState) {
        let t = tools();
        let manager = Arc::new(TerminalManager::default());
        manager.open_route("route", 80, 24).unwrap();
        let events = Arc::new(Mutex::new(Vec::<StepEvent>::new()));
        let output = Arc::new(Mutex::new(Vec::<u8>::new()));
        let (ev, out) = (events.clone(), output.clone());
        let runner = Runner {
            manager,
            route: "route".into(),
            ssh: t.ssh.clone(),
            scp: t.scp.clone(),
            source: matches!(spec.source, Endpoint::Remote { .. }).then(|| conn("src")),
            dest: matches!(spec.destination, Endpoint::Remote { .. }).then(|| conn("dst")),
            on_data: Channel::new(move |b| {
                if let InvokeResponseBody::Raw(bytes) = b {
                    out.lock().unwrap().extend(bytes);
                }
                Ok(())
            }),
            on_step: Channel::new(move |b| {
                if let InvokeResponseBody::Json(j) = b {
                    ev.lock().unwrap().push(serde_json::from_str(&j).unwrap());
                }
                Ok(())
            }),
        };
        let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        let mut state = RunState::new(id, spec);
        // What `folder_open` resolves: commands run as typed by the login shell.
        state.before_cmd = state.spec.before.as_ref().and_then(|h| h.command.clone());
        state.after_cmd = state.spec.after.as_ref().and_then(|h| h.command.clone());
        let code = runner.execute(&mut state);
        let text = String::from_utf8_lossy(&output.lock().unwrap()).into_owned();
        let mut last = vec![StepStatus::Running; state.steps.len()];
        for e in events.lock().unwrap().iter() {
            last[e.index] = e.status;
        }
        (code, text, last, state)
    }

    fn all_done(statuses: &[StepStatus]) -> bool {
        statuses.iter().all(|s| *s == StepStatus::Done)
    }

    #[test]
    fn server_to_pc() {
        let home = tempfile::tempdir().unwrap();
        let src = tree(home.path());
        let local = tempfile::tempdir().unwrap();
        std::fs::create_dir(local.path().join("site")).unwrap();
        let mut s = spec(
            remote_end(&src.display().to_string()),
            local_end(local.path()),
        );
        s.unpack = true;
        s.keep_zip = false;
        let (code, text, statuses, state) = execute(s);
        assert_eq!(code, Some(0), "{text}");
        assert!(all_done(&statuses), "{statuses:?}\n{text}");
        assert!(text.contains("Checksum OK"), "{text}");
        let site = local.path().join("site");
        assert_eq!(
            std::fs::read(site.join("index.html")).unwrap(),
            b"<h1>hi</h1>"
        );
        let mode = std::fs::metadata(site.join("deploy.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o750);
        let names: Vec<String> = std::fs::read_dir(local.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().any(|n| n.starts_with("site.bak-")),
            "{names:?}"
        );
        assert!(!names.iter().any(|n| n.ends_with(".zip")), "{names:?}");
        assert!(!Path::new(&remote_archive(&state.id)).exists());
    }

    #[test]
    fn pc_to_server_with_owner_and_existing_folder() {
        let local = tempfile::tempdir().unwrap();
        let src = tree(local.path());
        let server = tempfile::tempdir().unwrap();
        let parent = server.path().join("www");
        std::fs::create_dir_all(parent.join("site")).unwrap();
        std::fs::write(parent.join("site").join("old.txt"), b"old").unwrap();
        let mut s = spec(local_end(&src), remote_end(&parent.display().to_string()));
        // chown to yourself always works.
        let me = String::from_utf8(Command::new("id").arg("-un").output().unwrap().stdout).unwrap();
        s.owner = Some(me.trim().to_string());
        let (code, text, statuses, state) = execute(s);
        assert_eq!(code, Some(0), "{text}");
        assert!(all_done(&statuses), "{statuses:?}\n{text}");
        assert_eq!(
            std::fs::read(parent.join("site").join("assets").join("my file.css")).unwrap(),
            b"body{}"
        );
        assert!(!parent.join("site").join("old.txt").exists(), "moved aside");
        assert!(text.contains("Checksum OK"), "{text}");
        // Temporary archives are gone on both sides.
        assert!(!Path::new(&remote_inbox(&state.id)).exists());
        assert!(!state.local_zip.unwrap().exists());
    }

    #[test]
    fn server_to_server_keeping_a_local_copy() {
        let a = tempfile::tempdir().unwrap();
        let src = tree(a.path());
        let b = tempfile::tempdir().unwrap();
        let copy = tempfile::tempdir().unwrap();
        let mut s = spec(
            remote_end(&src.display().to_string()),
            remote_end(&b.path().join("dest").display().to_string()),
        );
        s.keep_copy = Some(copy.path().display().to_string());
        let (code, text, statuses, state) = execute(s);
        assert_eq!(code, Some(0), "{text}");
        assert!(all_done(&statuses), "{statuses:?}\n{text}");
        assert_eq!(
            std::fs::read(b.path().join("dest").join("site").join("index.html")).unwrap(),
            b"<h1>hi</h1>"
        );
        assert!(copy.path().join("site.zip").is_file(), "the kept copy");
        assert!(!Path::new(&remote_archive(&state.id)).exists());
        assert!(!Path::new(&remote_inbox(&state.id)).exists());
    }

    #[test]
    fn scripts_around_a_server_to_server_copy_with_excludes() {
        let a = tempfile::tempdir().unwrap();
        let src = tree(a.path());
        std::fs::create_dir_all(src.join("node_modules").join("x")).unwrap();
        std::fs::write(src.join("node_modules").join("x").join("big.js"), b"x").unwrap();
        let b = tempfile::tempdir().unwrap();
        let marks = tempfile::tempdir().unwrap();
        let before = marks.path().join("before");
        let after = marks.path().join("after");
        let mut s = spec(
            remote_end(&src.display().to_string()),
            remote_end(&b.path().display().to_string()),
        );
        s.excludes = vec!["node_modules".into(), "*.css".into()];
        s.before = Some(Hook {
            script_id: None,
            command: Some(format!("touch '{}'", before.display())),
        });
        // Runs after unpacking: the copy is already there.
        s.after = Some(Hook {
            script_id: None,
            command: Some(format!(
                "test -f '{}/site/index.html' && touch '{}'",
                b.path().display(),
                after.display()
            )),
        });
        let (code, text, statuses, _) = execute(s);
        assert_eq!(code, Some(0), "{text}");
        assert!(all_done(&statuses), "{statuses:?}\n{text}");
        assert!(before.exists() && after.exists(), "{text}");
        let site = b.path().join("site");
        assert!(site.join("index.html").exists());
        assert!(!site.join("node_modules").exists(), "excluded");
        assert!(
            !site.join("assets").join("my file.css").exists(),
            "excluded"
        );
    }

    #[test]
    fn a_failing_script_stops_the_transfer() {
        let a = tempfile::tempdir().unwrap();
        let src = tree(a.path());
        let local = tempfile::tempdir().unwrap();
        let mut s = spec(
            remote_end(&src.display().to_string()),
            local_end(local.path()),
        );
        s.before = Some(Hook {
            script_id: None,
            command: Some("exit 3".into()),
        });
        let (code, text, statuses, _) = execute(s);
        assert_eq!(code, Some(1), "{text}");
        assert_eq!(statuses[1], StepStatus::Failed, "{text}");
        assert!(
            text.contains("script before failed with exit code 3"),
            "{text}"
        );
    }

    #[test]
    fn stop_on_existing_folder_fails_before_touching_anything() {
        let local = tempfile::tempdir().unwrap();
        let src = tree(local.path());
        let server = tempfile::tempdir().unwrap();
        std::fs::create_dir(server.path().join("site")).unwrap();
        let mut s = spec(
            local_end(&src),
            remote_end(&server.path().display().to_string()),
        );
        s.existing = Existing::Stop;
        let (code, text, statuses, _) = execute(s);
        assert_eq!(code, Some(1), "{text}");
        assert_eq!(
            statuses[1],
            StepStatus::Failed,
            "the destination check: {text}"
        );
    }
}
