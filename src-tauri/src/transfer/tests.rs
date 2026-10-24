use std::path::Path;

use super::*;
use crate::db::Db;
use crate::history::revert;

fn host(alias: &str, group_id: Option<String>) -> HostInput {
    HostInput {
        alias: alias.into(),
        hostname: Some(format!("{alias}.example.com")),
        user: Some("deploy".into()),
        port: Some(22),
        group_id,
        description: format!("{alias} server"),
        tags: vec!["prod".into()],
        fields: vec![HostField {
            key: "Owner".into(),
            value: "ops".into(),
        }],
        ..Default::default()
    }
}

fn group(tx: &Transaction, name: &str, parent: Option<String>) -> AppResult<String> {
    Ok(groups::create(
        tx,
        &GroupInput {
            name: name.into(),
            parent_id: parent,
            color: None,
            icon: None,
        },
        Source::User,
    )?
    .id)
}

fn script(name: &str, content: &str) -> ScriptInput {
    ScriptInput {
        name: name.into(),
        description: String::new(),
        content: content.into(),
        interpreter: "bash".into(),
        default_args: String::new(),
        run_mode: RunMode::Stream,
    }
}

/// A source database: Prod/DB/{db1, db2}, web (no group), one script.
fn source_db() -> (Db, Vec<String>, String) {
    let db = Db::open_in_memory().unwrap();
    let (hosts, script_id) = db
        .tx(|tx| {
            let prod = group(tx, "Prod", None)?;
            let dbg = group(tx, "DB", Some(prod))?;
            group(tx, "Unused", None)?;
            let ids = vec![
                hosts::create(tx, host("db1", Some(dbg.clone())), Source::User)?.id,
                hosts::create(tx, host("db2", Some(dbg)), Source::User)?.id,
                hosts::create(tx, host("web", None), Source::User)?.id,
            ];
            let s = scripts::create(tx, script("Disk", "df -h"), Source::User)?;
            Ok((ids, s.id))
        })
        .unwrap();
    (db, hosts, script_id)
}

fn loaded(doc: Document) -> Loaded {
    Loaded {
        kind: "export",
        file_name: "x.rsexport".into(),
        doc,
        warnings: Vec::new(),
    }
}

fn choose(p: &ImportPreview, f: impl Fn(&HostPreview) -> Strategy) -> ImportRequest {
    ImportRequest {
        hosts: p
            .hosts
            .iter()
            .map(|h| Choice {
                key: h.key.clone(),
                strategy: f(h),
            })
            .collect(),
        scripts: p
            .scripts
            .iter()
            .map(|s| Choice {
                key: s.key.clone(),
                strategy: s.suggested,
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn export_round_trip_through_encrypted_file() {
    let (db, ids, script_id) = source_db();
    let home = tempfile::tempdir().unwrap();
    let sel = Selection {
        host_ids: vec![ids[0].clone(), ids[2].clone()],
        script_ids: vec![script_id],
        include_keys: false,
    };
    let (doc, warnings) = db.tx(|tx| build(tx, &sel, home.path())).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(doc.hosts.len(), 2);
    // db1's group and its parent, not the unused group.
    let mut names: Vec<_> = doc.groups.iter().map(|g| g.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["DB", "Prod"]);
    assert_eq!(doc.hosts[0].tags, ["prod"]);

    let file = home.path().join("out.rsexport");
    let pw = SecretString::from("export password".to_string());
    write_export(&file, &doc, &pw, 2).unwrap();
    let bytes = std::fs::read(&file).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("db1.example.com"));

    let source = ImportSource::Export {
        path: file.display().to_string(),
        password: "export password".into(),
    };
    let l = load(&source).unwrap();
    assert_eq!(l.doc, doc);
    assert_eq!(l.file_name, "out.rsexport");

    let wrong = ImportSource::Export {
        path: file.display().to_string(),
        password: "nope nope".into(),
    };
    let err = load(&wrong).err().unwrap().to_string();
    assert!(err.contains("wrong password"), "{err}");

    assert!(
        db.tx(|tx| build(tx, &Selection::default(), home.path()))
            .is_err()
    );
    let gone = Selection {
        host_ids: vec!["missing".into()],
        ..Default::default()
    };
    assert!(db.tx(|tx| build(tx, &gone, home.path())).is_err());
}

#[test]
fn export_path_adds_extension() {
    let base = std::env::temp_dir();
    let p = export_path(&base.join("hosts").display().to_string(), EXTENSION).unwrap();
    assert_eq!(p.file_name().unwrap(), "hosts.rsexport");
    let p = export_path(&base.join("a.RSEXPORT").display().to_string(), EXTENSION).unwrap();
    assert_eq!(p.file_name().unwrap(), "a.RSEXPORT");
    assert!(export_path("relative.rsexport", EXTENSION).is_err());
}

#[test]
fn import_into_empty_db_rebuilds_groups_and_marks_pending() {
    let (src, ids, script_id) = source_db();
    let home = tempfile::tempdir().unwrap();
    let sel = Selection {
        host_ids: ids.clone(),
        script_ids: vec![script_id],
        include_keys: false,
    };
    let (doc, _) = src.tx(|tx| build(tx, &sel, home.path())).unwrap();
    let l = loaded(doc);

    let db = Db::open_in_memory().unwrap();
    let target = db.tx(|tx| group(tx, "Imported", None)).unwrap();
    let p = db.tx(|tx| preview(tx, &l, home.path())).unwrap();
    assert!(p.hosts.iter().all(|h| h.status == ItemStatus::New));
    let db1 = p.hosts.iter().find(|h| h.alias == "db1").unwrap();
    assert_eq!(db1.group_path, ["Prod", "DB"]);

    let mut req = choose(&p, |h| h.suggested);
    req.target_group_id = Some(target.clone());
    let r = db.tx(|tx| apply(tx, &l, &req)).unwrap();
    assert_eq!((r.created, r.groups_created, r.scripts), (3, 2, 1));
    // New hosts are not in ~/.ssh/config yet: all of them can be written there.
    assert_eq!(r.pending_host_ids.len(), 3);

    db.tx(|tx| {
        let all = hosts::list(tx)?;
        let db1 = all.iter().find(|h| h.alias == "db1").unwrap();
        let db2 = all.iter().find(|h| h.alias == "db2").unwrap();
        let web = all.iter().find(|h| h.alias == "web").unwrap();
        assert_eq!(db1.group_id, db2.group_id, "one DB group, reused");
        let dbg = groups::get(tx, db1.group_id.as_ref().unwrap())?;
        assert_eq!(dbg.name, "DB");
        let prod = groups::get(tx, dbg.parent_id.as_ref().unwrap())?;
        assert_eq!(
            (prod.name.as_str(), prod.parent_id.as_ref()),
            ("Prod", Some(&target))
        );
        assert_eq!(web.group_id.as_ref(), Some(&target));
        assert_eq!(db1.description, "db1 server");
        assert_eq!(db1.fields[0].value, "ops");
        assert_eq!(db1.sync_state, "db_only");
        Ok(())
    })
    .unwrap();

    // Importing again: everything is the same; groups are matched by name, not duplicated.
    let p = db.tx(|tx| preview(tx, &l, home.path())).unwrap();
    assert!(p.hosts.iter().all(|h| h.status == ItemStatus::Same));
    assert_eq!(p.scripts[0].status, ItemStatus::Same);
    let r = db
        .tx(|tx| apply(tx, &l, &choose(&p, |_| Strategy::Skip)))
        .unwrap();
    assert_eq!((r.created, r.skipped, r.groups_created), (0, 3, 0));
}

#[test]
fn merge_strategies_on_existing_hosts() {
    let (src, ids, _) = source_db();
    let home = tempfile::tempdir().unwrap();
    let sel = Selection {
        host_ids: ids,
        ..Default::default()
    };
    let (mut doc, _) = src.tx(|tx| build(tx, &sel, home.path())).unwrap();
    for h in &mut doc.hosts {
        h.port = Some(2222);
        h.notes = "from the other machine".into();
    }
    let l = loaded(doc);

    let db = Db::open_in_memory().unwrap();
    let mine = db.tx(|tx| group(tx, "Mine", None)).unwrap();
    let (db1_id, db2_id) = db
        .tx(|tx| {
            let a = hosts::create(tx, host("db1", Some(mine.clone())), Source::User)?;
            let b = hosts::create(tx, host("db2", None), Source::User)?;
            // Pretend db1 is in ~/.ssh/config as it is now.
            let snap = serde_json::to_string(&a.connection())?;
            tx.execute(
                "UPDATE hosts SET sync_state = 'synced', config_snapshot = ?2 WHERE id = ?1",
                rusqlite::params![a.id, snap],
            )?;
            hosts::create(tx, host("web-2", None), Source::User)?;
            Ok((a.id, b.id))
        })
        .unwrap();

    let p = db.tx(|tx| preview(tx, &l, home.path())).unwrap();
    let by = |a: &str| p.hosts.iter().find(|h| h.alias == a).unwrap();
    assert_eq!(by("db1").status, ItemStatus::Differs);
    assert_eq!(by("db1").differences, ["port", "notes"]);
    assert_eq!(by("db1").existing_id.as_ref(), Some(&db1_id));
    assert_eq!(by("db2").copy_alias, "db2-2");
    assert_eq!(by("web").status, ItemStatus::New);
    // web-2 exists, so a copy of web would be web-3; web itself is new.
    assert_eq!(by("web").copy_alias, "web-3");

    // Import as new is not allowed for an existing host.
    let bad = choose(&p, |_| Strategy::Import);
    assert!(matches!(
        db.tx(|tx| apply(tx, &l, &bad)),
        Err(AppError::Conflict(_))
    ));

    let req = choose(&p, |h| match h.alias.as_str() {
        "db1" => Strategy::Overwrite,
        "db2" => Strategy::Copy,
        _ => Strategy::Skip,
    });
    let r = db.tx(|tx| apply(tx, &l, &req)).unwrap();
    assert_eq!((r.overwritten, r.copied, r.skipped), (1, 1, 1));
    db.tx(|tx| {
        let db1 = hosts::get(tx, &db1_id)?;
        assert_eq!(db1.port, Some(2222));
        assert_eq!(db1.notes, "from the other machine");
        assert_eq!(
            db1.group_id.as_ref(),
            Some(&mine),
            "overwrite keeps the local group"
        );
        // The config still says port 22: a pending update for the guarded writer.
        assert!(db1.config_dirty);
        assert_eq!(
            hosts::get(tx, &db2_id)?.port,
            Some(22),
            "copy leaves the original"
        );
        let copy = hosts::list(tx)?
            .into_iter()
            .find(|h| h.alias == "db2-2")
            .unwrap();
        assert_eq!(copy.port, Some(2222));
        assert!(r.pending_host_ids.contains(&db1_id) && r.pending_host_ids.contains(&copy.id));
        Ok(())
    })
    .unwrap();
}

#[test]
fn an_import_is_one_undoable_batch() {
    let (src, ids, script_id) = source_db();
    let home = tempfile::tempdir().unwrap();
    let sel = Selection {
        host_ids: ids,
        script_ids: vec![script_id],
        include_keys: false,
    };
    let (doc, _) = src.tx(|tx| build(tx, &sel, home.path())).unwrap();
    let l = loaded(doc);
    let db = Db::open_in_memory().unwrap();
    let p = db.tx(|tx| preview(tx, &l, home.path())).unwrap();
    db.tx(|tx| apply(tx, &l, &choose(&p, |h| h.suggested)))
        .unwrap();

    let batch: String = db
        .tx(|tx| {
            Ok(tx.query_row(
                "SELECT batch_id FROM audit_log WHERE source = 'import' LIMIT 1",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    db.tx(|tx| revert::revert_batch(tx, &batch)).unwrap();
    db.tx(|tx| {
        assert!(hosts::list(tx)?.iter().all(|h| h.deleted_at.is_some()));
        assert!(groups::list(tx)?.is_empty());
        assert!(scripts::list(tx)?.is_empty());
        Ok(())
    })
    .unwrap();
}

#[test]
fn plain_config_import() {
    let text = "\
Include ~/.ssh/conf.d/*
Host *
    ServerAliveInterval 30
Host jump
    HostName 10.0.0.1
    User admin
Host app
    HostName app.internal
    ProxyJump jump
    IdentityFile \"~/.ssh/my key\"
Host app
    HostName dup
";
    let l = from_config(Path::new("other-config"), text);
    let aliases: Vec<_> = l.doc.hosts.iter().map(|h| h.alias.as_str()).collect();
    assert_eq!(aliases, ["jump", "app"]);
    assert_eq!(
        l.doc.hosts[1].identity_file.as_deref(),
        Some("~/.ssh/my key")
    );
    let w = l.warnings.join("\n");
    assert!(
        w.contains("Include ~/.ssh/conf.d/* was not followed"),
        "{w}"
    );
    assert!(w.contains("cannot be imported: Host *"), "{w}");

    let db = Db::open_in_memory().unwrap();
    let home = tempfile::tempdir().unwrap();
    let p = db.tx(|tx| preview(tx, &l, home.path())).unwrap();
    assert_eq!(p.kind, "config");
    assert!(p.created_at.is_none());
    let r = db
        .tx(|tx| apply(tx, &l, &choose(&p, |h| h.suggested)))
        .unwrap();
    assert_eq!(r.created, 2);
    assert_eq!(r.pending_host_ids.len(), 2);
}

#[test]
fn config_snippet_is_a_valid_config() {
    let (db, ids, _) = source_db();
    let text = db.tx(|tx| config_snippet(tx, &ids)).unwrap();
    assert!(text.starts_with("# Exported from RidgeSSH"), "{text}");
    let parsed = ssh_config::parse_single(Path::new("x"), &text);
    let aliases: Vec<_> = parsed.hosts.iter().map(|h| h.alias.as_str()).collect();
    assert_eq!(aliases, ["db1", "db2", "web"]);
    assert_eq!(parsed.hosts[0].hostname.as_deref(), Some("db1.example.com"));
    assert!(db.tx(|tx| config_snippet(tx, &[])).is_err());
}

#[test]
fn key_files_opt_in_and_never_overwritten() {
    let src_home = tempfile::tempdir().unwrap();
    let ssh = src_home.path().join(".ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    std::fs::write(ssh.join("id_work"), "PRIVATE-WORK").unwrap();
    std::fs::write(ssh.join("id_work.pub"), "PUBLIC-WORK").unwrap();
    std::fs::write(ssh.join("id_other"), "PRIVATE-OTHER").unwrap();

    let db = Db::open_in_memory().unwrap();
    let ids = db
        .tx(|tx| {
            let mut out = Vec::new();
            for (alias, key) in [
                ("a", "~/.ssh/id_work"),
                ("b", "~/.ssh/id_work"),
                ("c", "~/.ssh/id_other"),
                ("d", "~/.ssh/missing"),
                ("e", "~/.ssh/%h_key"),
            ] {
                let mut h = host(alias, None);
                h.identity_file = Some(key.into());
                out.push(hosts::create(tx, h, Source::User)?.id);
            }
            Ok(out)
        })
        .unwrap();

    let mut sel = Selection {
        host_ids: ids,
        ..Default::default()
    };
    let (doc, _) = db.tx(|tx| build(tx, &sel, src_home.path())).unwrap();
    assert!(doc.keys.is_empty(), "keys are opt-in");
    sel.include_keys = true;
    let (mut doc, warnings) = db.tx(|tx| build(tx, &sel, src_home.path())).unwrap();
    assert_eq!(doc.keys.len(), 2, "deduplicated, missing skipped");
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    // A hostile file trying to escape ~/.ssh.
    doc.keys.push(ExportKey {
        identity_file: "~/.ssh/../AppData/evil".into(),
        private_key: "eA==".into(),
        public_key: None,
    });

    let home = tempfile::tempdir().unwrap();
    let dst = home.path().join(".ssh");
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(dst.join("id_other"), "SOMETHING ELSE").unwrap();

    let p = keys::preview(&doc.keys, home.path());
    let st: Vec<_> = p.iter().map(|k| k.status).collect();
    assert_eq!(
        st,
        [KeyStatus::New, KeyStatus::Exists, KeyStatus::Unsupported]
    );

    let (installed, warnings) = keys::install(&doc.keys, home.path());
    assert_eq!(installed, [dst.join("id_work").display().to_string()]);
    assert_eq!(
        std::fs::read_to_string(dst.join("id_work")).unwrap(),
        "PRIVATE-WORK"
    );
    assert_eq!(
        std::fs::read_to_string(dst.join("id_work.pub")).unwrap(),
        "PUBLIC-WORK"
    );
    assert_eq!(
        std::fs::read_to_string(dst.join("id_other")).unwrap(),
        "SOMETHING ELSE"
    );
    assert!(!home.path().join("AppData").exists());
    assert_eq!(warnings.len(), 1, "{warnings:?}");

    // Installing again changes nothing.
    let p = keys::preview(&doc.keys, home.path());
    assert_eq!(p[0].status, KeyStatus::Same);
    assert!(keys::install(&doc.keys, home.path()).0.is_empty());
}

/// Your real ~/.ssh/config (read-only): import into a scratch DB, export as a snippet, re-parse.
/// Run with `cargo test real_config_round_trip -- --ignored --nocapture`.
#[test]
#[ignore]
fn real_config_round_trip() {
    let path = crate::paths::default_ssh_config().unwrap();
    let text = ssh_config::read_text(&path).unwrap();
    let l = from_config(&path, &text);
    let db = Db::open_in_memory().unwrap();
    let home = tempfile::tempdir().unwrap();
    let p = db.tx(|tx| preview(tx, &l, home.path())).unwrap();
    let r = db
        .tx(|tx| apply(tx, &l, &choose(&p, |h| h.suggested)))
        .unwrap();
    let ids: Vec<String> = db
        .tx(|tx| Ok(hosts::list(tx)?.into_iter().map(|h| h.id).collect()))
        .unwrap();
    let snippet = db.tx(|tx| config_snippet(tx, &ids)).unwrap();
    let back = ssh_config::parse_single(Path::new("snippet"), &snippet);
    // Compare as JSON, keyed by alias.
    let key = |a: &str, v: serde_json::Value| (a.to_string(), v.to_string());
    let mut original: Vec<_> = l
        .doc
        .hosts
        .iter()
        .map(|h| {
            key(
                &h.alias,
                serde_json::json!([
                    h.hostname,
                    h.user,
                    h.port,
                    h.identity_file,
                    h.proxy_jump,
                    h.extra_options
                ]),
            )
        })
        .collect();
    let mut again: Vec<_> = back
        .hosts
        .iter()
        .map(|h| {
            key(
                &h.alias,
                serde_json::json!([
                    h.hostname,
                    h.user,
                    h.port,
                    h.identity_file,
                    h.proxy_jump,
                    h.extra_options
                ]),
            )
        })
        .collect();
    original.sort();
    again.sort();
    println!(
        "hosts {} imported {} warnings {:?}",
        l.doc.hosts.len(),
        r.created,
        l.warnings
    );
    assert_eq!(original, again);
}
