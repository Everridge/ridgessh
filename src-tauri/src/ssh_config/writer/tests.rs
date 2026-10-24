use std::collections::HashSet;

use super::*;
use crate::db::hosts::Directive;

fn file(text: &str) -> ConfigFile {
    ConfigFile::parse("config", text)
}

fn conn_of(text: &str, alias: &str) -> Connection {
    let f = file(text);
    let block = find_host_block(&f, alias).unwrap();
    extract_host(&f, block, alias, &mut Vec::new()).connection()
}

/// Applies the edits, runs the verifier and returns the new text.
fn run(text: &str, edits: &[Edit]) -> String {
    let f = file(text);
    let out = apply(&f, edits, &HashSet::new()).unwrap();
    verify(&f, &out, edits).unwrap();
    out
}

fn update(text: &str, alias: &str, change: impl FnOnce(&mut Connection)) -> String {
    let mut conn = conn_of(text, alias);
    change(&mut conn);
    run(
        text,
        &[Edit::Update {
            alias: alias.into(),
            conn,
        }],
    )
}

fn new_conn(alias: &str) -> Connection {
    Connection {
        alias: alias.into(),
        hostname: Some("10.0.0.9".into()),
        user: Some("deploy".into()),
        ..Default::default()
    }
}

const SAMPLE: &str = "\
# global
ServerAliveInterval 30

Host web
  HostName=10.0.0.1
  # the deploy user
  user deploy
  LocalForward 8080 localhost:80

Host db
\tHostName 10.0.0.2

# defaults for everything
Host *
  ForwardAgent no
";

#[test]
fn no_op_update_is_byte_identical() {
    assert_eq!(update(SAMPLE, "web", |_| {}), SAMPLE);
    assert_eq!(apply(&file(SAMPLE), &[], &HashSet::new()).unwrap(), SAMPLE);
}

#[test]
fn changes_only_the_edited_value() {
    let out = update(SAMPLE, "web", |c| c.hostname = Some("10.9.9.9".into()));
    assert_eq!(
        out,
        SAMPLE.replace("HostName=10.0.0.1", "HostName=10.9.9.9")
    );
}

#[test]
fn renames_the_header() {
    let out = update(SAMPLE, "web", |c| c.alias = "web-1".into());
    assert_eq!(out, SAMPLE.replace("Host web\n", "Host web-1\n"));
}

#[test]
fn removes_and_adds_known_fields_in_place() {
    let out = update(SAMPLE, "web", |c| {
        c.user = None;
        c.port = Some(2222);
    });
    // The new line takes the place of the last known field before it.
    assert_eq!(out, SAMPLE.replace("  user deploy\n", "  Port 2222\n"));
}

#[test]
fn rewritten_line_keeps_its_layout() {
    let text = "Host a\n  HostName=x   \n\tUSER   bob\t\n";
    let out = update(text, "a", |c| {
        c.hostname = Some("y".into());
        c.user = Some("alice".into());
    });
    assert_eq!(out, "Host a\n  HostName=y   \n\tUSER   alice\t\n");
}

#[test]
fn new_field_uses_block_indentation() {
    let out = update(SAMPLE, "db", |c| c.user = Some("pg".into()));
    assert!(
        out.contains("Host db\n\tHostName 10.0.0.2\n\tUser pg\n"),
        "{out}"
    );
}

#[test]
fn quotes_values_with_spaces() {
    let out = update(SAMPLE, "db", |c| {
        c.identity_file = Some(r"C:\Users\me\.ssh\my key".into())
    });
    assert!(
        out.contains("\tIdentityFile \"C:\\Users\\me\\.ssh\\my key\"\n"),
        "{out}"
    );
    let out = update(SAMPLE, "db", |c| {
        c.identity_file = Some(r"C:\Users\me\.ssh\id_ed25519".into())
    });
    assert!(
        out.contains("\tIdentityFile C:\\Users\\me\\.ssh\\id_ed25519\n"),
        "{out}"
    );
}

#[test]
fn extra_options_keep_unchanged_lines() {
    let text = "Host a\n  HostName h\n  # keep me\n  LocalForward 1 x:1\n  ForwardAgent yes\n  Compression yes\n";
    let out = update(text, "a", |c| {
        c.extra_options = vec![
            Directive {
                key: "LocalForward".into(),
                value: "1 x:1".into(),
            },
            Directive {
                key: "ServerAliveInterval".into(),
                value: "15".into(),
            },
            Directive {
                key: "Compression".into(),
                value: "no".into(),
            },
        ];
    });
    assert_eq!(
        out,
        "Host a\n  HostName h\n  # keep me\n  LocalForward 1 x:1\n  ServerAliveInterval 15\n  Compression no\n"
    );
}

#[test]
fn appends_extra_options_to_a_block_without_any() {
    let out = update(SAMPLE, "db", |c| {
        c.extra_options.push(Directive {
            key: "ForwardAgent".into(),
            value: "yes".into(),
        })
    });
    assert!(
        out.contains("Host db\n\tHostName 10.0.0.2\n\tForwardAgent yes\n\n# defaults"),
        "{out}"
    );
}

#[test]
fn add_goes_after_last_host_and_before_rules() {
    let out = run(
        SAMPLE,
        &[Edit::Add {
            conn: new_conn("api"),
        }],
    );
    assert!(
        out.contains(
            "Host db\n\tHostName 10.0.0.2\n\nHost api\n  HostName 10.0.0.9\n  User deploy\n\n# defaults for everything\nHost *\n"
        ),
        "{out}"
    );
}

#[test]
fn add_to_empty_or_missing_file() {
    assert_eq!(
        run(
            "",
            &[Edit::Add {
                conn: new_conn("api")
            }]
        ),
        "Host api\n    HostName 10.0.0.9\n    User deploy\n"
    );
}

#[test]
fn add_when_file_has_only_rules() {
    let text = "IdentitiesOnly yes\n\nHost *\n  ForwardAgent no\n";
    let out = run(
        text,
        &[Edit::Add {
            conn: new_conn("api"),
        }],
    );
    assert_eq!(
        out,
        "IdentitiesOnly yes\n\nHost api\n  HostName 10.0.0.9\n  User deploy\n\nHost *\n  ForwardAgent no\n"
    );
}

#[test]
fn add_to_file_without_trailing_newline() {
    let out = run(
        "Host a\n  HostName x",
        &[Edit::Add {
            conn: new_conn("b"),
        }],
    );
    assert_eq!(
        out,
        "Host a\n  HostName x\n\nHost b\n  HostName 10.0.0.9\n  User deploy"
    );
}

#[test]
fn remove_collapses_blank_lines_and_keeps_next_comment() {
    let out = run(SAMPLE, &[Edit::Remove { alias: "db".into() }]);
    assert_eq!(out, SAMPLE.replace("Host db\n\tHostName 10.0.0.2\n\n", ""));
}

#[test]
fn remove_last_block() {
    let text = "Host a\n  HostName x\n\nHost b\n  HostName y\n";
    assert_eq!(
        run(text, &[Edit::Remove { alias: "b".into() }]),
        "Host a\n  HostName x\n"
    );
}

#[test]
fn remove_and_add_together() {
    let text = "Host a\n  HostName x\n\nHost b\n  HostName y\n";
    let out = run(
        text,
        &[
            Edit::Remove { alias: "b".into() },
            Edit::Add {
                conn: new_conn("c"),
            },
        ],
    );
    assert_eq!(
        out,
        "Host a\n  HostName x\n\nHost c\n  HostName 10.0.0.9\n  User deploy\n"
    );
}

#[test]
fn keeps_crlf_bom_and_mixed_endings() {
    let text = "\u{feff}Host a\r\n  HostName x\r\n\r\nHost b\n  HostName y\r\n";
    let out = update(text, "a", |c| c.user = Some("u".into()));
    assert_eq!(
        out,
        "\u{feff}Host a\r\n  HostName x\r\n  User u\r\n\r\nHost b\n  HostName y\r\n"
    );
    let out = run(
        text,
        &[Edit::Add {
            conn: new_conn("c"),
        }],
    );
    assert!(
        out.ends_with("  HostName y\r\n\r\nHost c\r\n  HostName 10.0.0.9\r\n  User deploy\r\n"),
        "{out:?}"
    );
}

#[test]
fn refuses_directive_injection() {
    let f = file(SAMPLE);
    let none = HashSet::new();
    let mut c = new_conn("api");
    c.extra_options = vec![Directive {
        key: "Host".into(),
        value: "evil".into(),
    }];
    assert!(apply(&f, &[Edit::Add { conn: c.clone() }], &none).is_err());
    c.extra_options = vec![Directive {
        key: "Match".into(),
        value: "all".into(),
    }];
    assert!(apply(&f, &[Edit::Add { conn: c.clone() }], &none).is_err());
    c.extra_options.clear();
    c.hostname = Some("x\nHost evil".into());
    assert!(apply(&f, &[Edit::Add { conn: c.clone() }], &none).is_err());
    c.hostname = None;
    c.alias = "a b".into();
    assert!(apply(&f, &[Edit::Add { conn: c }], &none).is_err());
}

#[test]
fn refuses_duplicate_aliases() {
    let f = file(SAMPLE);
    let none = HashSet::new();
    assert!(
        apply(
            &f,
            &[Edit::Add {
                conn: new_conn("db")
            }],
            &none
        )
        .is_err()
    );
    let mut renamed = conn_of(SAMPLE, "web");
    renamed.alias = "db".into();
    let rename = Edit::Update {
        alias: "web".into(),
        conn: renamed,
    };
    assert!(apply(&f, &[rename], &none).is_err());
    let included: HashSet<String> = ["api".to_string()].into();
    assert!(
        apply(
            &f,
            &[Edit::Add {
                conn: new_conn("api")
            }],
            &included
        )
        .is_err()
    );
    // A literal pattern inside a multi-pattern rule also counts.
    let f = file("Host api api.example.com\n  User x\n");
    assert!(
        apply(
            &f,
            &[Edit::Add {
                conn: new_conn("api")
            }],
            &none
        )
        .is_err()
    );
}

#[test]
fn missing_block_is_a_conflict() {
    let edit = Edit::Remove {
        alias: "nope".into(),
    };
    let err = apply(&file(SAMPLE), &[edit], &HashSet::new()).unwrap_err();
    assert!(matches!(err, AppError::Conflict(_)));
}

#[test]
fn verify_catches_unexpected_changes() {
    let f = file(SAMPLE);
    let edits = [Edit::Remove { alias: "db".into() }];
    let tampered = SAMPLE.replace("ForwardAgent no", "ForwardAgent yes");
    assert!(verify(&f, &tampered, &edits).is_err());
    let tampered = SAMPLE.replace("10.0.0.1", "10.0.0.7");
    assert!(verify(&f, &tampered, &[]).is_err());
}

/// Every host in the real ~/.ssh/config survives a no-op rewrite and a change-and-revert.
/// Read-only; run with `cargo test real_config_rewrites -- --ignored --nocapture`.
#[test]
#[ignore]
fn real_config_rewrites_losslessly() {
    let path = crate::paths::default_ssh_config().unwrap();
    let text = crate::ssh_config::read_text(&path).unwrap();
    let f = ConfigFile::parse(&path, &text);
    let parsed = crate::ssh_config::load(&path).unwrap();
    let none = HashSet::new();
    let mut n = 0;
    for h in parsed.hosts.iter().filter(|h| h.source == path) {
        let conn = h.connection();
        let same = [Edit::Update {
            alias: h.alias.clone(),
            conn: conn.clone(),
        }];
        assert_eq!(apply(&f, &same, &none).unwrap(), text, "{}", h.alias);

        let mut changed = conn.clone();
        changed.hostname = Some("changed.invalid".into());
        changed.port = Some(2201);
        let edits = [Edit::Update {
            alias: h.alias.clone(),
            conn: changed,
        }];
        let out = apply(&f, &edits, &none).unwrap();
        let new = verify(&f, &out, &edits).unwrap();
        let back = [Edit::Update {
            alias: h.alias.clone(),
            conn,
        }];
        let restored = apply(&new, &back, &none).unwrap();
        verify(&new, &restored, &back).unwrap();
        if restored != text {
            let diff: Vec<String> = restored
                .lines()
                .zip(text.lines())
                .filter(|(a, b)| a != b)
                .map(|(a, b)| format!("got {a:?} want {b:?}"))
                .take(3)
                .collect();
            panic!("{} changed after revert: {diff:?}", h.alias);
        }
        n += 1;
    }
    println!("{n} hosts rewritten and verified");
}

/// The real OpenSSH reads what the writer wrote, including quoted and escaped values.
#[test]
fn real_ssh_reads_written_values() {
    let Ok(ssh) = crate::terminal::ssh_args::find_tool("ssh") else {
        eprintln!("ssh not installed; skipping");
        return;
    };
    let identity = r#"C:\Program Files\keys\it's "q"\id"#;
    let conn = Connection {
        alias: "ridgessh-selftest".into(),
        hostname: Some("selftest.invalid".into()),
        user: Some("deploy".into()),
        port: Some(2201),
        identity_file: Some(identity.into()),
        proxy_jump: None,
        extra_options: vec![Directive {
            key: "LocalForward".into(),
            value: "15433 localhost:5432".into(),
        }],
    };
    let text = run("Host *\n  ServerAliveInterval 30\n", &[Edit::Add { conn }]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    std::fs::write(&path, &text).unwrap();
    let out = std::process::Command::new(ssh)
        .arg("-G")
        .arg("-F")
        .arg(&path)
        .arg("ridgessh-selftest")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let resolved = String::from_utf8_lossy(&out.stdout).to_lowercase();
    for want in [
        "hostname selftest.invalid",
        "user deploy",
        "port 2201",
        "serveraliveinterval 30",
        &format!("identityfile {}", identity.to_lowercase()),
    ] {
        assert!(
            resolved.lines().any(|l| l == want),
            "missing {want:?} in:\n{text}"
        );
    }
}
