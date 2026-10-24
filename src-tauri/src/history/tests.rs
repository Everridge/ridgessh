use std::path::Path;

use super::revert::{Target, revert_batch, revert_entry};
use super::*;
use crate::db::Db;
use crate::db::groups::{self, GroupInput};
use crate::db::hosts::{self, Host, HostField, HostInput};
use crate::settings::{self, Settings};
use crate::ssh_config::{ConfigFile, ParsedConfig};

fn input(alias: &str) -> HostInput {
    HostInput {
        alias: alias.into(),
        hostname: Some("10.0.0.1".into()),
        ..Default::default()
    }
}

fn create(db: &Db, i: HostInput) -> Host {
    db.tx(|tx| hosts::create(tx, i, Source::User)).unwrap()
}

fn get(db: &Db, id: &str) -> Host {
    db.tx(|tx| hosts::get(tx, id)).unwrap()
}

fn edit(db: &Db, id: &str, f: impl FnOnce(&mut HostInput)) -> Host {
    let h = get(db, id);
    let mut i = HostInput {
        alias: h.alias.clone(),
        hostname: h.hostname.clone(),
        user: h.user.clone(),
        port: h.port,
        group_id: h.group_id.clone(),
        description: h.description.clone(),
        notes: h.notes.clone(),
        tags: h.tags.iter().map(|t| t.name.clone()).collect(),
        fields: h.fields.clone(),
        ..Default::default()
    };
    f(&mut i);
    db.tx(|tx| hosts::update(tx, id, i, Source::User)).unwrap()
}

/// History entries of a host, newest first.
fn entries(db: &Db, id: &str) -> Vec<AuditEntry> {
    db.tx(|tx| for_entity(tx, EntityType::Host, id, 100))
        .unwrap()
}

fn latest(db: &Db) -> AuditEntry {
    db.tx(|tx| list(tx, &Filter::default())).unwrap().remove(0)
}

#[test]
fn one_transaction_is_one_batch() {
    let db = Db::open_in_memory().unwrap();
    let a = create(&db, input("a"));
    let b = create(&db, input("b"));
    db.tx(|tx| hosts::soft_delete(tx, &[a.id.clone(), b.id.clone()], Source::User))
        .unwrap();
    let all = db.tx(|tx| list(tx, &Filter::default())).unwrap();
    assert_eq!(all.len(), 4);
    assert_eq!(all[0].batch_id, all[1].batch_id);
    assert_ne!(all[1].batch_id, all[2].batch_id);
    assert_ne!(all[2].batch_id, all[3].batch_id);
    assert!(all.iter().all(|e| e.batch_id.is_some()));
    assert_eq!(all[0].label, "b");
}

#[test]
fn list_filters_and_pages() {
    let db = Db::open_in_memory().unwrap();
    let h = create(
        &db,
        HostInput {
            notes: "rack 42_b".into(),
            ..input("web")
        },
    );
    create(&db, input("db"));
    db.tx(|tx| {
        groups::create(
            tx,
            &GroupInput {
                name: "Prod".into(),
                parent_id: None,
                color: None,
                icon: None,
            },
            Source::User,
        )
    })
    .unwrap();

    let q = |f: Filter| db.tx(|tx| list(tx, &f)).unwrap();
    assert_eq!(
        q(Filter {
            entity_type: Some(EntityType::Group),
            ..Default::default()
        })
        .len(),
        1
    );
    assert_eq!(
        q(Filter {
            source: Some(Source::Sync),
            ..Default::default()
        })
        .len(),
        0
    );
    let found = q(Filter {
        search: Some("42_b".into()),
        ..Default::default()
    });
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].entity_id, h.id);
    // `_` is literal, not a wildcard.
    assert!(
        q(Filter {
            search: Some("42xb".into()),
            ..Default::default()
        })
        .is_empty()
    );
    assert_eq!(
        q(Filter {
            search: Some(h.id.clone()),
            ..Default::default()
        })
        .len(),
        1
    );

    let page1 = q(Filter {
        limit: Some(2),
        ..Default::default()
    });
    let page2 = q(Filter {
        limit: Some(2),
        before_id: Some(page1[1].id),
        ..Default::default()
    });
    assert_eq!(page1.len(), 2);
    assert_eq!(page2.len(), 1);
    assert_eq!(page2[0].label, "web");
}

#[test]
fn undo_reverts_only_the_fields_of_that_change() {
    let db = Db::open_in_memory().unwrap();
    let h = create(&db, input("web"));
    edit(&db, &h.id, |i| i.description = "first".into());
    let change = entries(&db, &h.id)[0].id;
    edit(&db, &h.id, |i| i.notes = "later note".into());

    db.tx(|tx| revert_entry(tx, change, Target::Before))
        .unwrap();
    let after = get(&db, &h.id);
    assert_eq!(after.description, "");
    assert_eq!(after.notes, "later note", "later edits survive");
    assert_eq!(entries(&db, &h.id)[0].source, "restore");

    // "Restore this version" goes back to the full snapshot.
    db.tx(|tx| revert_entry(tx, change, Target::After)).unwrap();
    let v = get(&db, &h.id);
    assert_eq!((v.description.as_str(), v.notes.as_str()), ("first", ""));
}

#[test]
fn undo_create_trashes_and_undo_delete_restores() {
    let db = Db::open_in_memory().unwrap();
    let h = create(&db, input("web"));
    let created = entries(&db, &h.id)[0].id;
    db.tx(|tx| revert_entry(tx, created, Target::Before))
        .unwrap();
    assert!(get(&db, &h.id).deleted_at.is_some());

    let trashed = entries(&db, &h.id)[0].id;
    db.tx(|tx| revert_entry(tx, trashed, Target::Before))
        .unwrap();
    assert!(get(&db, &h.id).deleted_at.is_none());
}

#[test]
fn undo_purge_brings_the_host_back() {
    let db = Db::open_in_memory().unwrap();
    let h = create(
        &db,
        HostInput {
            tags: vec!["prod".into()],
            fields: vec![HostField {
                key: "owner".into(),
                value: "ops".into(),
            }],
            notes: "keep me".into(),
            ..input("web")
        },
    );
    db.tx(|tx| hosts::soft_delete(tx, std::slice::from_ref(&h.id), Source::User))
        .unwrap();
    db.tx(|tx| hosts::purge(tx, &h.id, Source::User)).unwrap();
    assert!(db.tx(|tx| hosts::get(tx, &h.id)).is_err());

    let purge = latest(&db);
    assert_eq!(purge.action, "purge");
    db.tx(|tx| revert_entry(tx, purge.id, Target::Before))
        .unwrap();
    let back = get(&db, &h.id);
    assert_eq!(back.alias, "web");
    assert_eq!(back.notes, "keep me");
    assert_eq!(back.tags[0].name, "prod");
    assert_eq!(back.fields.len(), 1);
    assert_eq!(back.sync_state, "db_only");
    // It comes back as it was right before the purge: in trash.
    assert!(back.deleted_at.is_some());
}

#[test]
fn undo_group_delete_moves_everything_back() {
    let db = Db::open_in_memory().unwrap();
    let g = |name: &str, parent: Option<&str>| GroupInput {
        name: name.into(),
        parent_id: parent.map(Into::into),
        color: None,
        icon: None,
    };
    let prod = db
        .tx(|tx| groups::create(tx, &g("Prod", None), Source::User))
        .unwrap();
    let eu = db
        .tx(|tx| groups::create(tx, &g("EU", Some(&prod.id)), Source::User))
        .unwrap();
    let h = create(
        &db,
        HostInput {
            group_id: Some(prod.id.clone()),
            ..input("web")
        },
    );

    db.tx(|tx| groups::delete(tx, &prod.id, Source::User))
        .unwrap();
    assert_eq!(get(&db, &h.id).group_id, None);
    let batch_id = latest(&db).batch_id.unwrap();

    let n = db.tx(|tx| revert_batch(tx, &batch_id)).unwrap();
    assert_eq!(n, 3);
    assert_eq!(db.tx(|tx| groups::get(tx, &prod.id)).unwrap().name, "Prod");
    assert_eq!(get(&db, &h.id).group_id.as_deref(), Some(prod.id.as_str()));
    assert_eq!(
        db.tx(|tx| groups::get(tx, &eu.id))
            .unwrap()
            .parent_id
            .as_deref(),
        Some(prod.id.as_str())
    );
}

#[test]
fn undo_settings_change() {
    let db = Db::open_in_memory().unwrap();
    db.tx(|tx| {
        settings::save(
            tx,
            Settings {
                confirmation_word: "WRITE".into(),
                ..Settings::default()
            },
        )
    })
    .unwrap();
    let e = db
        .tx(|tx| {
            list(
                tx,
                &Filter {
                    entity_type: Some(EntityType::Settings),
                    ..Default::default()
                },
            )
        })
        .unwrap()
        .remove(0);
    assert_eq!(e.label, "Settings");
    db.tx(|tx| revert_entry(tx, e.id, Target::Before)).unwrap();
    assert_eq!(db.tx(settings::confirmation_word).unwrap(), "CONFIRM");
}

#[test]
fn undoing_a_scan_change_only_marks_a_pending_config_edit() {
    let db = Db::open_in_memory().unwrap();
    let scan = |text: &str| {
        let file = ConfigFile::parse("/c", text);
        let mut p = ParsedConfig::default();
        for b in &file.blocks {
            crate::ssh_config::collect_block_for_tests(&file, b, &mut p);
        }
        db.tx(|tx| crate::sync::run(tx, &p, Path::new("/c")))
            .unwrap();
    };
    scan("Host web\n  HostName 10.0.0.1\n");
    scan("Host web\n  HostName 10.0.0.2\n");
    let web = db.tx(hosts::list).unwrap().remove(0);
    assert_eq!(web.hostname.as_deref(), Some("10.0.0.2"));

    let sync_change = entries(&db, &web.id)[0].clone();
    assert_eq!(sync_change.source, "sync");
    db.tx(|tx| revert_entry(tx, sync_change.id, Target::Before))
        .unwrap();
    let after = get(&db, &web.id);
    assert_eq!(after.hostname.as_deref(), Some("10.0.0.1"));
    assert!(
        after.config_dirty,
        "the config file is only written through the guarded writer"
    );
}

#[test]
fn tags_and_backups_cannot_be_undone() {
    let db = Db::open_in_memory().unwrap();
    db.tx(|tx| {
        record(
            tx,
            Source::User,
            EntityType::Backup,
            "f.rsbak",
            Action::Restore,
            None::<&()>,
            Some(&1),
        )
    })
    .unwrap();
    let e = latest(&db);
    assert!(!revert::supported(&e));
    assert!(db.tx(|tx| revert_entry(tx, e.id, Target::Before)).is_err());
}

#[test]
fn purge_trash_skips_hosts_still_in_config() {
    let db = Db::open_in_memory().unwrap();
    let a = create(&db, input("a"));
    let b = create(&db, input("b"));
    db.tx(|tx| {
        tx.execute(
            "UPDATE hosts SET config_snapshot = '{\"alias\":\"b\",\"extraOptions\":[]}', sync_state = 'synced' WHERE id = ?1",
            [&b.id],
        )?;
        hosts::soft_delete(tx, &[a.id.clone(), b.id.clone()], Source::User)
    })
    .unwrap();

    // Nothing is older than a cutoff in the past.
    let (purged, _) = db
        .tx(|tx| hosts::purge_trash(tx, Some("2000-01-01T00:00:00.000Z"), Source::System))
        .unwrap();
    assert!(purged.is_empty());

    let (purged, kept) = db
        .tx(|tx| hosts::purge_trash(tx, None, Source::User))
        .unwrap();
    assert_eq!((purged, kept), (vec!["a".to_string()], 1));
    assert!(db.tx(|tx| hosts::get(tx, &b.id)).is_ok());
}
