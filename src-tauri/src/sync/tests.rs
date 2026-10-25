use std::path::{Path, PathBuf};

use super::*;
use crate::db::Db;
use crate::db::hosts::HostInput;
use crate::ssh_config::ConfigFile;

const MAIN: &str = "/home/me/.ssh/config";

fn parsed(text: &str) -> ParsedConfig {
    let file = ConfigFile::parse(MAIN, text);
    let mut out = ParsedConfig::default();
    for block in &file.blocks {
        crate::ssh_config::collect_block_for_tests(&file, block, &mut out);
    }
    out.files.push(file);
    crate::ssh_config::attach_user_matches(&mut out);
    out
}

fn scan(db: &Db, text: &str) -> SyncReport {
    db.tx(|tx| run(tx, &parsed(text), Path::new(MAIN))).unwrap()
}

fn host(db: &Db, alias: &str) -> Host {
    db.tx(|tx| {
        Ok(hosts::list(tx)?
            .into_iter()
            .find(|h| h.alias == alias && h.deleted_at.is_none()))
    })
    .unwrap()
    .unwrap_or_else(|| panic!("no live host {alias}"))
}

fn edit(db: &Db, alias: &str, f: impl FnOnce(&mut HostInput)) -> Host {
    let h = host(db, alias);
    let mut input = HostInput {
        alias: h.alias.clone(),
        hostname: h.hostname.clone(),
        user: h.user.clone(),
        port: h.port,
        identity_file: h.identity_file.clone(),
        proxy_jump: h.proxy_jump.clone(),
        extra_options: h.extra_options.clone(),
        user_matches: h.user_matches.clone(),
        group_id: h.group_id.clone(),
        description: h.description.clone(),
        notes: h.notes.clone(),
        ..Default::default()
    };
    f(&mut input);
    db.tx(|tx| hosts::update(tx, &h.id, input, Source::User))
        .unwrap()
}

const TWO: &str = "Host a\n  HostName 10.0.0.1\n  User root\nHost b\n  HostName 10.0.0.2\n";

#[test]
fn first_scan_adds_hosts_as_new() {
    let db = Db::open_in_memory().unwrap();
    let r = scan(&db, TWO);
    assert_eq!(r.added, ["a", "b"]);
    let a = host(&db, "a");
    assert_eq!(a.sync_state, "config_only_new");
    assert_eq!(a.hostname.as_deref(), Some("10.0.0.1"));
    assert_eq!(a.config_snapshot.as_ref().unwrap().alias, "a");
    assert!(!a.config_dirty);
}

#[test]
fn rescan_without_changes_is_a_no_op() {
    let db = Db::open_in_memory().unwrap();
    scan(&db, TWO);
    let before = host(&db, "a");
    let r = scan(&db, TWO);
    assert!(!r.changed(), "{r:?}");
    let after = host(&db, "a");
    assert_eq!(after.updated_at, before.updated_at);
    assert_eq!(after.sync_state, "config_only_new");
}

#[test]
fn external_config_change_updates_db() {
    let db = Db::open_in_memory().unwrap();
    scan(&db, TWO);
    let id = host(&db, "a").id;
    db.tx(|tx| acknowledge(tx, std::slice::from_ref(&id)))
        .unwrap();
    let r = scan(&db, &TWO.replace("10.0.0.1", "10.9.9.9"));
    assert_eq!(r.updated, ["a"]);
    let a = host(&db, "a");
    assert_eq!(a.hostname.as_deref(), Some("10.9.9.9"));
    assert_eq!(a.sync_state, "synced");
}

#[test]
fn pending_edit_plus_config_change_is_a_conflict() {
    let db = Db::open_in_memory().unwrap();
    scan(&db, TWO);
    let edited = edit(&db, "a", |i| i.port = Some(2222));
    assert!(edited.config_dirty);

    // Rescan with an unchanged config: the pending edit survives, no conflict.
    let r = scan(&db, TWO);
    assert!(!r.changed());
    assert_eq!(host(&db, "a").port, Some(2222));

    let r = scan(&db, &TWO.replace("User root", "User admin"));
    assert_eq!(r.conflicts, ["a"]);
    let a = host(&db, "a");
    assert_eq!(a.sync_state, "conflict");
    assert_eq!(a.port, Some(2222), "DB keeps the user's version");
    assert_eq!(
        a.config_snapshot.as_ref().unwrap().user.as_deref(),
        Some("admin")
    );

    let resolved = db
        .tx(|tx| resolve_conflict(tx, &a.id, Keep::Config))
        .unwrap();
    assert_eq!(
        (resolved.port, resolved.user.as_deref()),
        (None, Some("admin"))
    );
    assert_eq!(resolved.sync_state, "synced");
    assert!(!resolved.config_dirty);
}

#[test]
fn keeping_db_version_leaves_a_pending_change() {
    let db = Db::open_in_memory().unwrap();
    scan(&db, TWO);
    edit(&db, "a", |i| i.port = Some(2222));
    scan(&db, &TWO.replace("User root", "User admin"));
    let id = host(&db, "a").id;
    let a = db
        .tx(|tx| resolve_conflict(tx, &id, Keep::Database))
        .unwrap();
    assert_eq!(
        (a.sync_state.as_str(), a.config_dirty, a.port),
        ("synced", true, Some(2222))
    );
}

#[test]
fn renaming_in_the_app_does_not_duplicate_on_rescan() {
    let db = Db::open_in_memory().unwrap();
    scan(&db, TWO);
    edit(&db, "a", |i| i.alias = "a-renamed".into());
    let r = scan(&db, TWO);
    assert!(r.added.is_empty() && r.removed.is_empty(), "{r:?}");
    let h = host(&db, "a-renamed");
    assert!(h.config_dirty);
}

#[test]
fn removed_block_is_flagged_and_can_come_back() {
    let db = Db::open_in_memory().unwrap();
    scan(&db, TWO);
    let r = scan(&db, "Host a\n  HostName 10.0.0.1\n  User root\n");
    assert_eq!(r.removed, ["b"]);
    let b = host(&db, "b");
    assert_eq!(b.sync_state, "removed_from_config");
    assert!(b.removed_from_config_at.is_some());

    let r = scan(&db, TWO);
    assert_eq!(r.reappeared, ["b"]);
    assert_eq!(host(&db, "b").sync_state, "synced");
}

#[test]
fn keep_db_only_stops_tracking() {
    let db = Db::open_in_memory().unwrap();
    scan(&db, TWO);
    scan(&db, "Host a\n");
    let id = host(&db, "b").id;
    db.tx(|tx| keep_db_only(tx, std::slice::from_ref(&id)))
        .unwrap();
    let b = host(&db, "b");
    assert_eq!(b.sync_state, "db_only");
    assert!(b.config_snapshot.is_none());
    assert!(!scan(&db, "Host a\n").removed.contains(&"b".to_string()));
}

#[test]
fn trashed_hosts_are_not_reimported() {
    let db = Db::open_in_memory().unwrap();
    scan(&db, TWO);
    let id = host(&db, "b").id;
    db.tx(|tx| hosts::soft_delete(tx, std::slice::from_ref(&id), Source::User))
        .unwrap();
    let r = scan(&db, TWO);
    assert!(r.added.is_empty(), "{r:?}");
}

#[test]
fn db_only_host_links_to_matching_config_block() {
    let db = Db::open_in_memory().unwrap();
    db.tx(|tx| {
        hosts::create(
            tx,
            HostInput {
                alias: "b".into(),
                hostname: Some("10.0.0.2".into()),
                ..Default::default()
            },
            Source::User,
        )
    })
    .unwrap();
    db.tx(|tx| {
        hosts::create(
            tx,
            HostInput {
                alias: "a".into(),
                hostname: Some("other".into()),
                ..Default::default()
            },
            Source::User,
        )
    })
    .unwrap();
    let r = scan(&db, TWO);
    assert!(r.added.is_empty());
    assert_eq!(host(&db, "b").sync_state, "synced");
    assert_eq!(r.conflicts, ["a"]);
}

#[test]
fn suggests_and_accepts_renames() {
    let db = Db::open_in_memory().unwrap();
    scan(&db, TWO);
    edit(&db, "b", |i| i.notes = "important box".into());
    // `b` renamed to `b2` directly in the config file.
    scan(&db, &TWO.replace("Host b", "Host b2"));

    let s = db.tx(rename_suggestions).unwrap();
    assert_eq!(
        s,
        [RenameSuggestion {
            from_id: host(&db, "b").id,
            to_id: host(&db, "b2").id
        }]
    );

    let merged = db
        .tx(|tx| accept_rename(tx, &s[0].from_id, &s[0].to_id))
        .unwrap();
    assert_eq!(merged.notes, "important box");
    assert_eq!(merged.sync_state, "synced");
    assert!(db.tx(rename_suggestions).unwrap().is_empty());
    let old = db.tx(|tx| hosts::get(tx, &s[0].from_id)).unwrap();
    assert!(old.deleted_at.is_some());
}

#[test]
fn hosts_from_included_files_record_their_source() {
    let db = Db::open_in_memory().unwrap();
    let mut p = parsed(TWO);
    p.hosts[1].source = PathBuf::from("/home/me/.ssh/conf.d/b.conf");
    db.tx(|tx| run(tx, &p, Path::new(MAIN))).unwrap();
    assert_eq!(host(&db, "a").config_source, None);
    assert!(host(&db, "b").config_source.unwrap().ends_with("b.conf"));
}

#[test]
fn sync_writes_audit_entries() {
    let db = Db::open_in_memory().unwrap();
    scan(&db, TWO);
    let a = host(&db, "a");
    let log = db
        .tx(|tx| history::for_entity(tx, EntityType::Host, &a.id, 10))
        .unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(
        (log[0].source.as_str(), log[0].action.as_str()),
        ("sync", "create")
    );
}

const USERS: &str =
    "Host a\n  HostName 10.0.0.1\nMatch originalhost a user deploy\n  IdentityFile ~/.ssh/a\n";

#[test]
fn per_user_blocks_sync_like_the_rest_of_the_connection() {
    let db = Db::open_in_memory().unwrap();
    scan(&db, USERS);
    let h = host(&db, "a");
    assert_eq!(h.user_matches.len(), 1);
    assert_eq!(h.user_matches[0].user, "deploy");

    // Changed outside RidgeSSH: taken over like any connection value.
    let r = scan(&db, &USERS.replace("~/.ssh/a", "~/.ssh/b"));
    assert_eq!(r.updated, ["a"]);
    assert_eq!(
        host(&db, "a").user_matches[0].identity_file.as_deref(),
        Some("~/.ssh/b")
    );

    // Edited in the app: a pending change until written.
    assert!(edit(&db, "a", |i| i.user_matches.clear()).config_dirty);
}

#[test]
fn hosts_scanned_before_user_blocks_were_tracked_take_them_over() {
    let db = Db::open_in_memory().unwrap();
    scan(&db, USERS);
    edit(&db, "a", |i| i.port = Some(2222));
    // As an older version saved it: no users in the host or its snapshot.
    db.tx(|tx| {
        let h = hosts::list(tx)?.remove(0);
        let mut snapshot = h.config_snapshot.clone().unwrap();
        snapshot.user_matches.clear();
        tx.execute(
            "UPDATE hosts SET user_matches = '[]', config_snapshot = ?2 WHERE id = ?1",
            params![h.id, serde_json::to_string(&snapshot)?],
        )?;
        Ok(())
    })
    .unwrap();
    assert!(host(&db, "a").user_matches.is_empty());

    let r = scan(&db, USERS);
    assert!(!r.changed(), "no conflict, nothing to review: {r:?}");
    let h = host(&db, "a");
    assert_eq!(h.user_matches.len(), 1);
    assert_eq!(h.config_snapshot.unwrap().user_matches.len(), 1);
    assert!(h.config_dirty, "the pending port edit stays");
    assert_eq!(h.sync_state, "config_only_new", "unchanged");
}
