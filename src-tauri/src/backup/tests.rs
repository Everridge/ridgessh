use std::fs;

use super::password::MemoryStore;
use super::restore::{self, RestoreOptions};
use super::*;
use crate::db::hosts::{self, HostInput};
use crate::history::{self as hist, EntityType, Source};
use crate::sync;

const CONFIG: &str = "Include extra\n\nHost web\n  HostName 10.0.0.1\n";
const EXTRA: &str = "Host inc\n  HostName 10.0.0.9\n";

struct Env {
    _dir: tempfile::TempDir,
    db: Db,
    paths: AppPaths,
    secrets: MemoryStore,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            data_dir: dir.path().join("data"),
            db_file: dir.path().join("data/ridgessh.db"),
            default_backup_dir: dir.path().join("backups"),
            ssh_config: dir.path().join(".ssh/config"),
        };
        fs::create_dir_all(paths.ssh_config.parent().unwrap()).unwrap();
        fs::write(&paths.ssh_config, CONFIG).unwrap();
        fs::write(dir.path().join(".ssh/extra"), EXTRA).unwrap();
        let env = Self {
            _dir: dir,
            db: Db::open_in_memory().unwrap(),
            paths,
            secrets: MemoryStore::default(),
        };
        env.scan();
        env
    }

    fn ctx(&self) -> Ctx<'_> {
        Ctx {
            db: &self.db,
            paths: &self.paths,
            secrets: &self.secrets,
            work_factor: 4,
        }
    }

    fn scan(&self) -> sync::SyncReport {
        let parsed = ssh_config::load(&self.paths.ssh_config).unwrap();
        self.db
            .tx(|tx| sync::run(tx, &parsed, &self.paths.ssh_config))
            .unwrap()
    }

    fn add_host(&self, alias: &str) -> hosts::Host {
        self.db
            .tx(|tx| {
                hosts::create(
                    tx,
                    HostInput {
                        alias: alias.into(),
                        hostname: Some("192.0.2.1".into()),
                        ..Default::default()
                    },
                    Source::User,
                )
            })
            .unwrap()
    }

    fn aliases(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .db
            .tx(hosts::list)
            .unwrap()
            .into_iter()
            .filter(|h| h.deleted_at.is_none())
            .map(|h| h.alias)
            .collect();
        v.sort();
        v
    }

    fn backups(&self) -> Vec<BackupInfo> {
        list(&self.paths.default_backup_dir).unwrap()
    }
}

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_string())
}

#[test]
fn file_names_round_trip() {
    let ts = DateTime::parse_from_rfc3339("2026-10-24T13:54:00.123Z")
        .unwrap()
        .to_utc();
    let auto = file_name(ts, false, Reason::ConfigWrite);
    assert_eq!(
        auto,
        "ridgessh-20261024T135400.123Z-auto-config-write.rsbak"
    );
    assert_eq!(parse_name(&auto), Some((ts, false, "config-write".into())));
    let manual = file_name(ts, true, Reason::Manual);
    assert_eq!(manual, "ridgessh-20261024T135400.123Z-manual.rsbak");
    assert_eq!(parse_name(&manual), Some((ts, true, "manual".into())));
    assert_eq!(parse_name("ridgessh-bogus-manual.rsbak"), None);
    assert_eq!(parse_name("notes.txt"), None);
}

#[test]
fn backup_contains_db_and_config_files() {
    let env = Env::new();
    let info = create(&env.ctx(), &pw("correct horse"), Reason::Manual).unwrap();
    assert!(info.manual);
    assert_eq!(env.backups(), std::slice::from_ref(&info));

    let bundle = Bundle::read(Path::new(&info.path), &pw("correct horse")).unwrap();
    assert_eq!(bundle.manifest.host_count, 2);
    assert_eq!(bundle.main_config(), Some(CONFIG.as_bytes()));
    let names: Vec<&str> = bundle
        .manifest
        .files
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(names, ["db.sqlite", "ssh/config", "ssh/include-0-extra"]);
    assert_eq!(bundle.files["ssh/include-0-extra"], EXTRA.as_bytes());

    // Not readable as a zip without the password.
    let raw = fs::read(&info.path).unwrap();
    assert!(!raw.windows(CONFIG.len()).any(|w| w == CONFIG.as_bytes()));
}

#[test]
fn wrong_password_and_damage_are_reported() {
    let env = Env::new();
    let info = create(&env.ctx(), &pw("correct horse"), Reason::Manual).unwrap();
    let err = Bundle::read(Path::new(&info.path), &pw("wrong horse"))
        .err()
        .unwrap();
    assert!(err.to_string().contains("wrong password"), "{err}");

    let mut raw = fs::read(&info.path).unwrap();
    let n = raw.len();
    raw[n - 20] ^= 0xff;
    fs::write(&info.path, &raw).unwrap();
    assert!(Bundle::read(Path::new(&info.path), &pw("correct horse")).is_err());

    fs::write(&info.path, b"plain text").unwrap();
    let err = Bundle::read(Path::new(&info.path), &pw("x")).err().unwrap();
    assert!(err.to_string().contains("not a RidgeSSH backup"), "{err}");
}

#[test]
fn retention_prunes_only_old_automatic_backups() {
    let dir = tempfile::tempdir().unwrap();
    let base = Utc::now();
    for i in 0..5 {
        let ts = base - chrono::Duration::days(i);
        fs::write(
            dir.path().join(file_name(ts, false, Reason::Periodic)),
            b"x",
        )
        .unwrap();
    }
    let manual = file_name(base - chrono::Duration::days(100), true, Reason::Manual);
    fs::write(dir.path().join(&manual), b"x").unwrap();
    fs::write(dir.path().join("renamed-by-me.rsbak"), b"x").unwrap();

    let s = Settings {
        retention_count: 3,
        ..Settings::default()
    };
    assert_eq!(prune(dir.path(), &s).unwrap(), 2);
    let left = list(dir.path()).unwrap();
    assert_eq!(left.iter().filter(|b| !b.manual).count(), 3);
    assert!(left.iter().any(|b| b.file_name == manual));
    assert!(left.iter().any(|b| b.reason == "unknown"));

    // Age limit: only the newest automatic one survives, manual ones always do.
    let s = Settings {
        retention_count: 30,
        retention_days: 1,
        ..Settings::default()
    };
    prune(dir.path(), &s).unwrap();
    assert_eq!(
        list(dir.path())
            .unwrap()
            .iter()
            .filter(|b| !b.manual)
            .count(),
        1
    );
    assert_eq!(list(dir.path()).unwrap().len(), 3);
}

#[test]
fn automatic_backups_need_a_password_and_respect_the_interval() {
    let env = Env::new();
    let ctx = env.ctx();
    assert_eq!(
        auto::before_change(&ctx, Reason::ConfigWrite).unwrap(),
        None
    );

    env.secrets.set("remembered pw").unwrap();
    let first = auto::before_change(&ctx, Reason::Periodic)
        .unwrap()
        .unwrap();
    assert!(!first.manual);
    assert_eq!(first.reason, "periodic");
    // Within the interval ordinary edits don't back up again; risky changes always do.
    assert_eq!(auto::before_change(&ctx, Reason::Periodic).unwrap(), None);
    assert!(
        auto::before_change(&ctx, Reason::ConfigWrite)
            .unwrap()
            .is_some()
    );
    assert_eq!(env.backups().len(), 2);
    Bundle::read(Path::new(&first.path), &pw("remembered pw")).unwrap();

    env.db
        .tx(|tx| {
            settings::save(
                tx,
                Settings {
                    auto_backup: false,
                    ..Settings::default()
                },
            )
        })
        .unwrap();
    assert_eq!(
        auto::before_change(&ctx, Reason::ConfigWrite).unwrap(),
        None
    );
}

#[test]
fn inspect_shows_what_restore_changes() {
    let env = Env::new();
    let info = create(&env.ctx(), &pw("correct horse"), Reason::Manual).unwrap();

    env.add_host("added-later");
    fs::write(
        &env.paths.ssh_config,
        CONFIG.replace("10.0.0.1", "10.0.0.2"),
    )
    .unwrap();
    env.scan();

    let i = restore::inspect(&env.ctx(), Path::new(&info.path), &pw("correct horse")).unwrap();
    assert_eq!(i.hosts.total, 2);
    assert_eq!(i.hosts.removed, ["added-later"]);
    assert_eq!(i.hosts.changed, ["web"]);
    assert!(i.hosts.added.is_empty());
    let config = i.config.unwrap();
    assert_eq!((config.additions, config.deletions), (1, 1));
    assert!(
        config
            .diff
            .iter()
            .any(|l| l.kind == "add" && l.text.contains("10.0.0.1"))
    );
    assert_eq!(i.confirmation_word, "CONFIRM");
}

#[test]
fn restore_database_and_config() {
    let env = Env::new();
    env.secrets.set("remembered pw").unwrap();
    let info = create(&env.ctx(), &pw("old password"), Reason::Manual).unwrap();

    env.add_host("added-later");
    let edited = CONFIG.replace("10.0.0.1", "10.0.0.2");
    fs::write(&env.paths.ssh_config, &edited).unwrap();
    env.scan();
    assert_eq!(env.aliases(), ["added-later", "inc", "web"]);

    let path = Path::new(&info.path);
    let password = pw("old password");
    let preview = restore::inspect(&env.ctx(), path, &password).unwrap();
    let base_hash = preview.config.unwrap().base_hash;

    // The config part needs the word and an unchanged file.
    let mut opts = RestoreOptions {
        restore_db: true,
        restore_config: true,
        config_base_hash: Some(base_hash.clone()),
        confirmation: Some("nope".into()),
    };
    assert!(restore::restore(&env.ctx(), path, &password, &opts).is_err());
    opts.confirmation = Some("CONFIRM".into());
    opts.config_base_hash = Some("stale".into());
    assert!(restore::restore(&env.ctx(), path, &password, &opts).is_err());
    assert_eq!(
        env.backups().len(),
        1,
        "nothing happens when the checks fail"
    );

    opts.config_base_hash = Some(base_hash);
    let r = restore::restore(&env.ctx(), path, &password, &opts).unwrap();
    assert_eq!(fs::read_to_string(&env.paths.ssh_config).unwrap(), CONFIG);
    assert_eq!(env.aliases(), ["inc", "web"]);
    assert!(!r.report.changed(), "{:?}", r.report);
    assert_eq!(
        fs::read_to_string(r.previous_config.unwrap()).unwrap(),
        edited
    );

    // The safety backup uses the remembered password and holds the pre-restore state.
    assert_eq!(r.safety_backup.reason, "before-restore");
    let safety = Bundle::read(Path::new(&r.safety_backup.path), &pw("remembered pw")).unwrap();
    assert_eq!(safety.manifest.host_count, 3);
    assert_eq!(safety.main_config(), Some(edited.as_bytes()));

    let log = env
        .db
        .tx(|tx| hist::for_entity(tx, EntityType::Backup, &r.safety_backup.file_name, 5))
        .unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].source, "restore");
}

#[test]
fn restore_database_only_keeps_config() {
    let env = Env::new();
    let info = create(&env.ctx(), &pw("old password"), Reason::Manual).unwrap();
    env.add_host("added-later");
    let opts = RestoreOptions {
        restore_db: true,
        restore_config: false,
        config_base_hash: None,
        confirmation: None,
    };
    let r = restore::restore(
        &env.ctx(),
        Path::new(&info.path),
        &pw("old password"),
        &opts,
    )
    .unwrap();
    assert_eq!(env.aliases(), ["inc", "web"]);
    assert!(r.previous_config.is_none());
    assert_eq!(fs::read_to_string(&env.paths.ssh_config).unwrap(), CONFIG);
    // No remembered password: the safety backup uses the one entered for the restore.
    Bundle::read(Path::new(&r.safety_backup.path), &pw("old password")).unwrap();
    // The live connection keeps working after the restore.
    env.add_host("after-restore");
    assert_eq!(env.aliases(), ["after-restore", "inc", "web"]);
}

#[test]
fn delete_is_confined_to_the_folder() {
    let env = Env::new();
    let info = create(&env.ctx(), &pw("correct horse"), Reason::Manual).unwrap();
    let dir = &env.paths.default_backup_dir;
    assert!(delete(dir, "../config").is_err());
    assert!(delete(dir, "..\\x.rsbak").is_err());
    assert!(delete(dir, "missing.rsbak").is_err());
    delete(dir, &info.file_name).unwrap();
    assert!(env.backups().is_empty());
}

/// Real-strength scrypt; run with `cargo test real_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn real_cost_round_trip() {
    let t = std::time::Instant::now();
    let data = encrypt(b"payload", &pw("correct horse"), WORK_FACTOR).unwrap();
    let enc = t.elapsed();
    let t = std::time::Instant::now();
    assert_eq!(decrypt(&data, &pw("correct horse")).unwrap(), b"payload");
    println!("encrypt {enc:?}, decrypt {:?}", t.elapsed());
}

/// Uses the real OS credential store with a throwaway entry.
#[test]
#[ignore]
fn real_keyring_round_trip() {
    let entry = keyring::Entry::new("RidgeSSH-selftest", "selftest").unwrap();
    entry.set_password("s3cret pw").unwrap();
    assert_eq!(entry.get_password().unwrap(), "s3cret pw");
    entry.delete_credential().unwrap();
    assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
}
