//! Phase 3: excludes, sudo, scripts around the transfer.

use std::path::Path;
use std::process::Command;

use super::*;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[test]
fn exclude_patterns_match_at_any_depth_or_relative_to_the_folder() {
    assert_eq!(
        exclude_patterns(
            Some("site"),
            &s(&["node_modules", "/storage/cache/", "*.log"])
        ),
        s(&[
            "*/node_modules",
            "*/node_modules/*",
            "site/storage/cache",
            "site/storage/cache/*",
            "*/*.log",
            "*/*.log/*",
        ])
    );
    assert_eq!(
        exclude_patterns(None, &s(&["a/b"])),
        s(&["*/a/b", "*/a/b/*"])
    );
    for bad in ["", "-x", "../up", "a/../b", "[unclosed"] {
        assert!(check_excludes(&s(&[bad])).is_err(), "{bad:?}");
    }
    assert!(check_excludes(&s(&["node_modules", ".git", "*.log", "var/cache"])).is_ok());
}

#[test]
fn sudo_wraps_the_whole_command_and_keeps_the_login_home() {
    let cmd = check_command("~/app").unwrap();
    // The login shell expands $HOME before sudo runs, so ~ is the login user's home.
    assert!(cmd.contains(r#" ridgessh "$HOME" '~/app'"#), "{cmd}");
    let wrapped = with_sudo(&cmd);
    assert!(
        wrapped.starts_with("sudo -p '[sudo] password for %u on %h: ' sh -c "),
        "{wrapped}"
    );
}

#[test]
fn hooks_add_steps_and_are_validated() {
    use StepKind::*;
    let tmp = tempfile::tempdir().unwrap();
    let mut spec = FolderSpec {
        source: Endpoint::Remote {
            host_id: "a".into(),
            user: None,
            path: "/srv/app".into(),
        },
        destination: Endpoint::Remote {
            host_id: "b".into(),
            user: None,
            path: "/srv".into(),
        },
        unpack: false,
        keep_zip: true,
        keep_copy: None,
        existing: Existing::Backup,
        owner: None,
        excludes: vec![],
        sudo_source: true,
        sudo_dest: true,
        before: Some(Hook {
            script_id: None,
            command: Some("systemctl stop app".into()),
        }),
        after: Some(Hook {
            script_id: Some("s1".into()),
            command: None,
        }),
    };
    assert!(check_spec(&spec).is_ok());
    assert_eq!(
        steps(&spec),
        [
            Check,
            CheckDest,
            Before,
            Pack,
            Download,
            Upload,
            UnpackRemote,
            After,
            Cleanup
        ]
    );

    let mut empty = spec.clone();
    empty.before = Some(Hook {
        script_id: None,
        command: Some("  ".into()),
    });
    assert!(check_spec(&empty).is_err());

    // An "after" script needs a destination server; sudo needs servers.
    spec.destination = Endpoint::Local {
        path: tmp.path().display().to_string(),
    };
    assert!(check_spec(&spec).is_err());
    spec.after = None;
    assert!(
        check_spec(&spec).is_err(),
        "sudo_dest without a destination server"
    );
    spec.sudo_dest = false;
    assert!(check_spec(&spec).is_ok());
}

fn tree(root: &Path) -> std::path::PathBuf {
    let dir = root.join("site");
    for d in [
        "node_modules/pkg",
        "src/node_modules",
        "storage/cache",
        "storage/app",
        "logs",
    ] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    for (f, body) in [
        ("index.php", "x"),
        ("node_modules/pkg/i.js", "x"),
        ("src/node_modules/deep.js", "x"),
        ("storage/cache/c.bin", "x"),
        ("storage/app/keep.txt", "x"),
        ("logs/today.log", "x"),
        ("debug.log", "x"),
    ] {
        std::fs::write(dir.join(f), body).unwrap();
    }
    dir
}

const EXCLUDES: [&str; 3] = ["node_modules", "storage/cache", "*.log"];

fn names(zip: &Path) -> Vec<String> {
    let mut a = ::zip::ZipArchive::new(std::fs::File::open(zip).unwrap()).unwrap();
    let mut n: Vec<String> = (0..a.len())
        .map(|i| {
            a.by_index(i)
                .unwrap()
                .name()
                .unwrap()
                .trim_start_matches("./")
                .to_string()
        })
        .collect();
    n.sort();
    n
}

/// What every packer must produce for [`tree`] with [`EXCLUDES`].
fn assert_excluded(names: &[String]) {
    for gone in ["node_modules", "cache", ".log", "deep.js"] {
        assert!(
            !names.iter().any(|n| n.contains(gone)),
            "{gone} in {names:?}"
        );
    }
    for kept in [
        "site/index.php",
        "site/storage/app/keep.txt",
        "site/logs/",
        "site/src/",
    ] {
        assert!(names.iter().any(|n| n == kept), "{kept} not in {names:?}");
    }
}

#[test]
fn local_packer_excludes() {
    let root = tempfile::tempdir().unwrap();
    let dir = tree(root.path());
    let out = root.path().join("o.zip");
    let report = local::pack(&dir, &out, &exclude_patterns(Some("site"), &s(&EXCLUDES))).unwrap();
    assert_excluded(&names(&out));
    assert!(report.excluded >= 4, "{report:?}");
}

#[test]
fn remote_packer_excludes() {
    let Ok(sh) = which::which("sh") else { return };
    let have_zip = which::which("zip").is_ok();
    let have_py = Command::new("python3")
        .args(["-c", "print(1)"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !have_zip && !have_py {
        eprintln!("neither zip nor python3; skipping");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let dir = tree(root.path());
    let out = root.path().join("o.zip");
    let cmd = pack_command(
        &dir.display().to_string().replace('\\', "/"),
        &out.display().to_string().replace('\\', "/"),
        &exclude_patterns(Some("site"), &s(&EXCLUDES)),
    )
    .unwrap();
    let o = Command::new(&sh)
        .arg("-c")
        .arg(&cmd)
        .env("HOME", root.path())
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert_excluded(&names(&out));

    // The python packer on its own (zip, when present, was used above).
    if have_py {
        let py_out = root.path().join("py.zip");
        let mut args = vec!["-c".to_string(), PYZIP.to_string(), "site".into()];
        args.push(py_out.display().to_string());
        args.extend(exclude_patterns(Some("site"), &s(&EXCLUDES)));
        let o = Command::new("python3")
            .args(&args)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        assert_excluded(&names(&py_out));
    }
}
