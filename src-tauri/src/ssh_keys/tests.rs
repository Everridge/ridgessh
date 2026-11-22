use super::*;
use crate::db::Db;

fn keygen() -> Option<PathBuf> {
    find_tool("ssh-keygen").ok()
}

fn input(alias: &str, identity_file: &str) -> HostInput {
    HostInput {
        alias: alias.into(),
        hostname: Some("10.0.0.9".into()),
        identity_file: Some(identity_file.into()),
        ..Default::default()
    }
}

#[test]
fn generate_writes_a_new_pair_and_never_overwrites() {
    if keygen().is_none() {
        eprintln!("ssh-keygen not installed; skipping");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let public = generate("~/.ssh/web_ed25519", "web@ridgessh", home.path()).unwrap();
    assert!(public.starts_with("ssh-ed25519 "), "{public}");
    assert!(public.ends_with(" web@ridgessh"), "{public}");
    let private = home.path().join(".ssh").join("web_ed25519");
    assert!(private.is_file());
    assert!(
        std::fs::read_to_string(&private)
            .unwrap()
            .contains("OPENSSH PRIVATE KEY")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&private).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "{mode:o}");
    }
    assert_eq!(
        read_public_key("~/.ssh/web_ed25519", home.path()).unwrap(),
        public
    );

    let again = generate("~/.ssh/web_ed25519", "web@ridgessh", home.path());
    assert!(matches!(again, Err(AppError::Conflict(_))), "{again:?}");
}

#[test]
fn generate_refuses_paths_outside_ssh_dir() {
    let home = tempfile::tempdir().unwrap();
    for bad in ["~/web_key", "~/.ssh/../web", "relative/key", "~/.ssh/%h"] {
        assert!(
            generate(bad, "c", home.path()).is_err(),
            "{bad} should be refused"
        );
    }
    // Only the .pub being there is enough to refuse.
    let ssh = home.path().join(".ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    std::fs::write(ssh.join("k.pub"), "x").unwrap();
    assert!(matches!(
        generate("~/.ssh/k", "c", home.path()),
        Err(AppError::Conflict(_))
    ));
}

#[test]
fn host_is_not_saved_when_the_key_cannot_be_written() {
    let db = Db::open_in_memory().unwrap();
    let home = tempfile::tempdir().unwrap();
    let ssh = home.path().join(".ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    std::fs::write(ssh.join("taken"), "x").unwrap();
    for identity in ["~/.ssh/taken", "~/elsewhere"] {
        let r = db.tx(|tx| create_host_with_key(tx, input("web", identity), home.path()));
        assert!(r.is_err(), "{identity}");
    }
    assert!(db.tx(hosts::list).unwrap().is_empty());
    // Nothing next to the existing file either.
    assert!(!ssh.join("taken.pub").exists());
}

#[test]
fn host_and_key_are_created_together() {
    if keygen().is_none() {
        return;
    }
    let db = Db::open_in_memory().unwrap();
    let home = tempfile::tempdir().unwrap();
    let (host, public) = db
        .tx(|tx| create_host_with_key(tx, input("web", "~/.ssh/web_ed25519"), home.path()))
        .unwrap();
    assert_eq!(host.identity_file.as_deref(), Some("~/.ssh/web_ed25519"));
    assert!(public.ends_with(" web@ridgessh"));
    assert_eq!(db.tx(hosts::list).unwrap().len(), 1);

    // A taken alias: refused before any key is written.
    let r = db.tx(|tx| create_host_with_key(tx, input("web", "~/.ssh/other"), home.path()));
    assert!(r.is_err());
    assert!(!home.path().join(".ssh").join("other").exists());
}

const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGx1c3Rlcg web@ridgessh";

#[test]
fn install_command_rejects_non_keys() {
    for bad in [
        "",
        "hello",
        "ssh-ed25519 AAA\nssh-rsa BBB",
        "-----BEGIN OPENSSH PRIVATE KEY-----",
    ] {
        assert!(install_command(bad).is_err(), "{bad:?}");
    }
    // One line, so csh/fish login shells pass it on unchanged.
    assert!(!install_command(KEY).unwrap().contains('\n'));
}

/// Runs the install command in a local POSIX shell with a temporary `$HOME`, like the remote
/// login shell would.
#[test]
fn install_command_runs_in_a_real_shell() {
    let Ok(sh) = which::which("sh") else {
        eprintln!("no sh; skipping");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let run = |key: &str| {
        let out = std::process::Command::new(&sh)
            .arg("-c")
            .arg(install_command(key).unwrap())
            .env("HOME", home.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let file = home.path().join(".ssh").join("authorized_keys");

    assert!(run(KEY).contains("Public key installed"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), format!("{KEY}\n"));

    // Safe to run twice.
    assert!(run(KEY).contains("already installed"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), format!("{KEY}\n"));

    // An existing file without a trailing newline keeps its last line intact.
    let tricky = "ssh-rsa AAAAB3Nza it's \"quoted\" $HOME";
    std::fs::write(&file, "ssh-rsa OTHER").unwrap();
    run(tricky);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        format!("ssh-rsa OTHER\n{tricky}\n")
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(&home.path().join(".ssh")), 0o700);
    }
}

#[test]
fn parses_ssh_g_output() {
    let t = parse_resolved(
        "user deploy\nhostname 10.0.0.5\nport 22\nuserknownhostsfile ~/.ssh/known_hosts ~/.ssh/known_hosts2\n",
    )
    .unwrap();
    assert_eq!(t.name, "10.0.0.5");
    assert_eq!(t.names(), ["10.0.0.5"]);
    assert_eq!(t.files, "~/.ssh/known_hosts ~/.ssh/known_hosts2");

    let t = parse_resolved("hostname db.example.com\nport 2222\nhostkeyalias db-key\n").unwrap();
    assert_eq!(t.names(), ["[db-key]:2222", "db-key"]);

    let t = parse_resolved("hostname x\nhostkeyalias none\n").unwrap();
    assert_eq!(t.name, "x");
    assert!(parse_resolved("port 22\n").is_none());
}

#[test]
fn known_hosts_paths_may_contain_spaces() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("First Last").join(".ssh");
    std::fs::create_dir_all(&dir).unwrap();
    let kh = dir.join("known_hosts");
    std::fs::write(&kh, "").unwrap();
    let ssh = home.path().join(".ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    std::fs::write(ssh.join("known_hosts"), "").unwrap();

    let printed = format!(
        "{} {}",
        kh.display(),
        dir.join("known_hosts2").display() // missing: skipped
    );
    assert_eq!(known_hosts_files(&printed, home.path()), [kh]);
    assert_eq!(
        known_hosts_files("~/.ssh/known_hosts ~/.ssh/known_hosts2", home.path()),
        [ssh.join("known_hosts")]
    );
    assert!(known_hosts_files("~/.ssh/%h", home.path()).is_empty());
}

/// Real `ssh-keygen -R` on a temporary known_hosts with made-up hosts.
#[test]
fn removes_stale_entries_with_ssh_keygen() {
    let Some(keygen) = keygen() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    // A real key, so ssh-keygen parses the lines.
    let public = generate("~/.ssh/hostkey", "c", dir.path()).unwrap();
    let blob = &public[..public.rfind(' ').unwrap()];
    let kh = dir.path().join("known_hosts");
    let line = |name: &str| format!("{name} {blob}\n");
    std::fs::write(
        &kh,
        format!(
            "{}{}{}",
            line("10.0.0.5"),
            line("[10.0.0.5]:2222"),
            line("keep.invalid")
        ),
    )
    .unwrap();

    let target = KnownHostsTarget {
        name: "10.0.0.5".into(),
        port: 2222,
        files: String::new(),
    };
    let removed = remove_entries(&keygen, &target.names(), std::slice::from_ref(&kh)).unwrap();
    assert_eq!(removed, ["[10.0.0.5]:2222", "10.0.0.5"]);
    let left = std::fs::read_to_string(&kh).unwrap();
    assert!(
        left.contains("keep.invalid") && !left.contains("10.0.0.5"),
        "{left}"
    );

    // Nothing left to remove: not an error.
    let removed = remove_entries(&keygen, &target.names(), &[kh]).unwrap();
    assert!(removed.is_empty());
}
