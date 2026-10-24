use std::fs;

use super::*;
use crate::db::Db;
use crate::db::hosts::HostInput;

struct Env {
    _dir: tempfile::TempDir,
    db: Db,
    paths: AppPaths,
}

impl Env {
    fn new(config: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data_dir: dir.path().join("data"),
            db_file: dir.path().join("data/ridgessh.db"),
            default_backup_dir: dir.path().join("data/backups"),
            ssh_config: dir.path().join(".ssh/config"),
        };
        fs::create_dir_all(paths.ssh_config.parent().unwrap()).unwrap();
        if let Some(text) = config {
            fs::write(&paths.ssh_config, text).unwrap();
        }
        let env = Self {
            _dir: dir,
            db: Db::open_in_memory().unwrap(),
            paths,
        };
        env.scan();
        env
    }

    fn cfg(&self) -> &Path {
        &self.paths.ssh_config
    }

    fn scan(&self) -> SyncReport {
        let parsed = ssh_config::load(self.cfg()).unwrap();
        self.db.tx(|tx| sync::run(tx, &parsed, self.cfg())).unwrap()
    }

    fn text(&self) -> String {
        fs::read_to_string(self.cfg()).unwrap()
    }

    fn host(&self, alias: &str) -> Host {
        self.db
            .tx(|tx| Ok(hosts::list(tx)?.into_iter().find(|h| h.alias == alias)))
            .unwrap()
            .unwrap_or_else(|| panic!("no host {alias}"))
    }

    fn edit(&self, alias: &str, f: impl FnOnce(&mut HostInput)) -> Host {
        let h = self.host(alias);
        let mut input = HostInput {
            alias: h.alias.clone(),
            hostname: h.hostname.clone(),
            user: h.user.clone(),
            port: h.port,
            identity_file: h.identity_file.clone(),
            proxy_jump: h.proxy_jump.clone(),
            extra_options: h.extra_options.clone(),
            notes: h.notes.clone(),
            ..Default::default()
        };
        f(&mut input);
        self.db
            .tx(|tx| hosts::update(tx, &h.id, input, Source::User))
            .unwrap()
    }

    fn candidates(&self) -> Vec<Candidate> {
        self.db.tx(|tx| candidates(tx, self.cfg())).unwrap()
    }

    fn preview(&self, changes: &[Change]) -> AppResult<Preview> {
        self.db.tx(|tx| preview(tx, self.cfg(), changes))
    }

    fn apply_with(&self, changes: &[Change], p: &Preview, word: &str) -> AppResult<ApplyResult> {
        self.db.tx(|tx| {
            apply(
                tx,
                &self.paths,
                self.cfg(),
                ApplyRequest {
                    changes,
                    base_hash: &p.base_hash,
                    result_hash: &p.result_hash,
                    confirmation: word,
                },
            )
        })
    }

    /// Preview, then apply with the right word.
    fn write(&self, changes: &[Change]) -> ApplyResult {
        let p = self.preview(changes).unwrap();
        self.apply_with(changes, &p, "CONFIRM").unwrap()
    }
}

fn change(h: &Host, op: Op) -> Change {
    Change {
        host_id: h.id.clone(),
        op,
    }
}

const CONFIG: &str = "# mine\nHost web\n  HostName 10.0.0.1\n  User deploy\n\nHost db\n  HostName 10.0.0.2\n\nHost *\n  ServerAliveInterval 30\n";

#[test]
fn edited_host_is_written_and_back_in_sync() {
    let env = Env::new(Some(CONFIG));
    let h = env.edit("web", |i| i.port = Some(2222));
    assert!(h.config_dirty);

    let cands = env.candidates();
    assert_eq!(
        cands,
        [Candidate {
            host_id: h.id.clone(),
            alias: "web".into(),
            op: Op::Update,
            selected: true,
            blocked: None,
        }]
    );
    let changes = [change(&h, Op::Update)];
    let p = env.preview(&changes).unwrap();
    assert_eq!((p.additions, p.deletions), (1, 0));
    assert!(
        p.diff
            .iter()
            .any(|l| l.kind == "add" && l.text == "  Port 2222")
    );
    assert_eq!(env.text(), CONFIG, "preview must not write");

    let r = env.apply_with(&changes, &p, "CONFIRM").unwrap();
    assert_eq!(r.changed, 1);
    assert!(!r.report.changed(), "{:?}", r.report);
    assert_eq!(
        env.text(),
        CONFIG.replace("  User deploy\n", "  User deploy\n  Port 2222\n")
    );
    let after = env.host("web");
    assert!(!after.config_dirty);
    assert_eq!(after.sync_state, "synced");
    assert_eq!(after.config_snapshot.as_ref().unwrap().port, Some(2222));
    assert!(env.candidates().is_empty());
    assert!(!env.scan().changed());

    // The previous file is in the backup folder.
    let backup = fs::read_to_string(r.backup_path.unwrap()).unwrap();
    assert_eq!(backup, CONFIG);
}

#[test]
fn rename_keeps_identity_across_rescans() {
    let env = Env::new(Some(CONFIG));
    let h = env.edit("web", |i| i.alias = "web-prod".into());
    env.write(&[change(&h, Op::Update)]);
    assert!(env.text().contains("Host web-prod\n"));
    let report = env.scan();
    assert!(!report.changed(), "{report:?}");
    let after = env.host("web-prod");
    assert_eq!(after.id, h.id);
    assert_eq!(after.sync_state, "synced");
}

#[test]
fn wrong_word_or_stale_preview_writes_nothing() {
    let env = Env::new(Some(CONFIG));
    let h = env.edit("web", |i| i.port = Some(2222));
    let changes = [change(&h, Op::Update)];
    let p = env.preview(&changes).unwrap();

    let err = env.apply_with(&changes, &p, "confirm").unwrap_err();
    assert!(matches!(err, AppError::Validation(_)));

    // Edited elsewhere after the preview (e.g. in an editor).
    let external = CONFIG.replace("# mine", "# mine, edited");
    fs::write(env.cfg(), &external).unwrap();
    let err = env.apply_with(&changes, &p, "CONFIRM").unwrap_err();
    assert!(matches!(err, AppError::Conflict(_)), "{err:?}");
    assert_eq!(env.text(), external);

    // The DB changed after the preview: the result would differ from what was shown.
    let p = env.preview(&changes).unwrap();
    env.edit("web", |i| i.port = Some(2223));
    let err = env.apply_with(&changes, &p, "CONFIRM").unwrap_err();
    assert!(matches!(err, AppError::Conflict(_)), "{err:?}");
    assert_eq!(env.text(), external);
    assert!(env.host("web").config_dirty);
}

#[test]
fn block_changed_outside_since_scan_is_blocked() {
    let env = Env::new(Some(CONFIG));
    let h = env.edit("web", |i| i.port = Some(2222));
    fs::write(env.cfg(), CONFIG.replace("User deploy", "User admin")).unwrap();
    let c = &env.candidates()[0];
    assert!(c.blocked.as_deref().unwrap().contains("Re-scan"), "{c:?}");
    assert!(!c.selected);
    assert!(env.preview(&[change(&h, Op::Update)]).is_err());
}

#[test]
fn custom_confirmation_word() {
    let env = Env::new(Some(CONFIG));
    env.db
        .tx(|tx| settings::set(tx, "confirmation_word", &"overwrite"))
        .unwrap();
    let h = env.edit("db", |i| i.user = Some("pg".into()));
    let changes = [change(&h, Op::Update)];
    let p = env.preview(&changes).unwrap();
    assert_eq!(p.confirmation_word, "overwrite");
    assert!(env.apply_with(&changes, &p, "CONFIRM").is_err());
    env.apply_with(&changes, &p, "overwrite").unwrap();
    assert!(
        env.text()
            .contains("Host db\n  HostName 10.0.0.2\n  User pg\n")
    );
}

#[test]
fn db_only_host_is_added_before_wildcards() {
    let env = Env::new(Some(CONFIG));
    let h = env
        .db
        .tx(|tx| {
            hosts::create(
                tx,
                HostInput {
                    alias: "api".into(),
                    hostname: Some("10.0.0.3".into()),
                    ..Default::default()
                },
                Source::User,
            )
        })
        .unwrap();
    let c = env.candidates();
    assert_eq!((c[0].op, c[0].selected), (Op::Add, false));
    env.write(&[change(&h, Op::Add)]);
    assert_eq!(
        env.text(),
        CONFIG.replace(
            "Host db\n  HostName 10.0.0.2\n\n",
            "Host db\n  HostName 10.0.0.2\n\nHost api\n  HostName 10.0.0.3\n\n"
        )
    );
    assert_eq!(env.host("api").sync_state, "synced");
    assert!(!env.scan().changed());
}

#[test]
fn removed_host_can_be_restored_into_config() {
    let env = Env::new(Some(CONFIG));
    fs::write(
        env.cfg(),
        CONFIG.replace("Host db\n  HostName 10.0.0.2\n\n", ""),
    )
    .unwrap();
    assert_eq!(env.scan().removed, ["db"]);
    let h = env.host("db");
    env.write(&[change(&h, Op::Add)]);
    assert!(env.text().contains("Host db\n  HostName 10.0.0.2\n"));
    assert_eq!(env.host("db").sync_state, "synced");
    assert!(!env.scan().changed());
}

#[test]
fn trashed_host_can_be_removed_from_config() {
    let env = Env::new(Some(CONFIG));
    let h = env.host("db");
    env.db
        .tx(|tx| hosts::soft_delete(tx, std::slice::from_ref(&h.id), Source::User))
        .unwrap();
    let c = env.candidates();
    assert_eq!((c.len(), c[0].op, c[0].selected), (1, Op::Remove, false));
    env.write(&[change(&h, Op::Remove)]);
    assert_eq!(
        env.text(),
        CONFIG.replace("Host db\n  HostName 10.0.0.2\n\n", "")
    );
    let after = env.host("db");
    assert_eq!(after.sync_state, "db_only");
    assert!(after.config_snapshot.is_none());
    assert!(env.candidates().is_empty());
    // Restoring it from trash later makes it a normal RidgeSSH-only host.
    assert!(!env.scan().changed());
}

#[test]
fn creates_missing_config() {
    let env = Env::new(None);
    let h = env
        .db
        .tx(|tx| {
            hosts::create(
                tx,
                HostInput {
                    alias: "first".into(),
                    hostname: Some("example.com".into()),
                    identity_file: Some(r"C:\Users\me\.ssh\my key".into()),
                    ..Default::default()
                },
                Source::User,
            )
        })
        .unwrap();
    let p = env.preview(&[change(&h, Op::Add)]).unwrap();
    assert!(!p.file_exists);
    let r = env.write(&[change(&h, Op::Add)]);
    assert!(r.backup_path.is_none());
    assert_eq!(
        env.text(),
        "Host first\n    HostName example.com\n    IdentityFile \"C:\\Users\\me\\.ssh\\my key\"\n"
    );
    assert_eq!(env.host("first").identity_file, h.identity_file);
    assert!(!env.scan().changed());
}

#[test]
fn conflict_and_included_hosts_are_blocked() {
    let env = Env::new(None);
    let ssh = env.cfg().parent().unwrap().to_path_buf();
    fs::write(ssh.join("extra"), "Host inc\n  HostName 10.1.1.1\n").unwrap();
    fs::write(
        env.cfg(),
        format!("Include {}\n\n{CONFIG}", ssh.join("extra").display()),
    )
    .unwrap();
    env.scan();
    let inc = env.edit("inc", |i| i.port = Some(2200));
    assert!(inc.config_dirty);

    env.edit("web", |i| i.port = Some(1));
    fs::write(env.cfg(), env.text().replace("User deploy", "User other")).unwrap();
    assert_eq!(env.scan().conflicts, ["web"]);

    let c = env.candidates();
    let reason = |alias: &str| {
        c.iter()
            .find(|c| c.alias == alias)
            .and_then(|c| c.blocked.clone())
            .unwrap()
    };
    assert!(reason("inc").contains("included file"));
    assert!(reason("web").contains("conflict"));
    assert!(env.preview(&[change(&inc, Op::Update)]).is_err());
}

#[test]
fn backups_are_pruned() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..KEEP_BACKUPS + 3 {
        let sub = dir.path().join("ssh-config");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join(format!("config-20200101-0000{i:02}.000")), "x").unwrap();
    }
    backup(dir.path(), b"new").unwrap();
    let n = fs::read_dir(dir.path().join("ssh-config")).unwrap().count();
    assert_eq!(n, KEEP_BACKUPS);
}
