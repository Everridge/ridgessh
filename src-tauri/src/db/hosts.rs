use std::collections::HashMap;

use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::db::tags::{self, Tag};
use crate::db::{is_unique_violation, new_id, now};
use crate::error::{AppError, AppResult};
use crate::history::{self, Action, EntityType, Source};
use crate::security::validate_alias;

/// One `Key value` line of an ssh_config Host block that has no dedicated column.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Directive {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostField {
    pub key: String,
    pub value: String,
}

/// One user's own settings for a host: a `Match originalhost <alias> user <user>` block in
/// ssh_config, used when connecting as `user@alias` (typically the user's own key).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct UserMatch {
    pub user: String,
    /// The block's first `IdentityFile`.
    pub identity_file: Option<String>,
    /// Every other line of the block, in config order.
    pub extra_options: Vec<Directive>,
}

/// The connection part of a host: what maps to its `Host` block in ssh_config, plus the
/// per-user `Match` blocks that belong to it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    pub proxy_jump: Option<String>,
    pub extra_options: Vec<Directive>,
    #[serde(default)]
    pub user_matches: Vec<UserMatch>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub id: String,
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    pub proxy_jump: Option<String>,
    pub extra_options: Vec<Directive>,
    pub user_matches: Vec<UserMatch>,
    pub group_id: Option<String>,
    pub description: String,
    pub notes: String,
    pub color: Option<String>,
    pub favorite: bool,
    pub sync_state: String,
    pub config_dirty: bool,
    /// What ~/.ssh/config said at the last scan; `None` if the host is not in the config.
    pub config_snapshot: Option<Connection>,
    /// Included file the host comes from; `None` for the main config file.
    pub config_source: Option<String>,
    /// Hash of the config block at the last scan; internal to sync.
    #[serde(skip)]
    pub config_fingerprint: Option<String>,
    pub last_seen_in_config_at: Option<String>,
    pub removed_from_config_at: Option<String>,
    pub last_connected_at: Option<String>,
    pub deleted_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub tags: Vec<Tag>,
    pub fields: Vec<HostField>,
}

/// Editable part of a host, as sent by the frontend for create/update.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HostInput {
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    pub proxy_jump: Option<String>,
    pub extra_options: Vec<Directive>,
    pub user_matches: Vec<UserMatch>,
    pub group_id: Option<String>,
    pub description: String,
    pub notes: String,
    pub color: Option<String>,
    pub favorite: bool,
    pub tags: Vec<String>,
    pub fields: Vec<HostField>,
}

const COLUMNS: &str = "id, alias, hostname, user, port, identity_file, proxy_jump, extra_options,
    group_id, description, notes, color, favorite, sync_state, config_dirty,
    last_seen_in_config_at, removed_from_config_at, last_connected_at, deleted_at,
    created_at, updated_at, config_snapshot, config_source, config_fingerprint, user_matches";

fn from_row(r: &Row) -> rusqlite::Result<Host> {
    let extra: String = r.get(7)?;
    Ok(Host {
        id: r.get(0)?,
        alias: r.get(1)?,
        hostname: r.get(2)?,
        user: r.get(3)?,
        port: r.get(4)?,
        identity_file: r.get(5)?,
        proxy_jump: r.get(6)?,
        extra_options: serde_json::from_str(&extra).unwrap_or_default(),
        user_matches: serde_json::from_str(&r.get::<_, String>(24)?).unwrap_or_default(),
        group_id: r.get(8)?,
        description: r.get(9)?,
        notes: r.get(10)?,
        color: r.get(11)?,
        favorite: r.get(12)?,
        sync_state: r.get(13)?,
        config_dirty: r.get(14)?,
        config_snapshot: r
            .get::<_, Option<String>>(21)?
            .and_then(|s| serde_json::from_str(&s).ok()),
        config_source: r.get(22)?,
        config_fingerprint: r.get(23)?,
        last_seen_in_config_at: r.get(15)?,
        removed_from_config_at: r.get(16)?,
        last_connected_at: r.get(17)?,
        deleted_at: r.get(18)?,
        created_at: r.get(19)?,
        updated_at: r.get(20)?,
        tags: Vec::new(),
        fields: Vec::new(),
    })
}

/// Loads tags and custom fields for `hosts` with two queries.
fn attach_relations(tx: &Transaction, hosts: &mut [Host]) -> AppResult<()> {
    let index: HashMap<String, usize> = hosts
        .iter()
        .enumerate()
        .map(|(i, h)| (h.id.clone(), i))
        .collect();

    let mut stmt = tx.prepare(
        "SELECT ht.host_id, t.id, t.name, t.color FROM host_tags ht
         JOIN tags t ON t.id = ht.tag_id ORDER BY t.name COLLATE NOCASE",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let host_id: String = r.get(0)?;
        if let Some(&i) = index.get(&host_id) {
            hosts[i].tags.push(Tag {
                id: r.get(1)?,
                name: r.get(2)?,
                color: r.get(3)?,
            });
        }
    }

    let mut stmt =
        tx.prepare("SELECT host_id, key, value FROM host_fields ORDER BY host_id, sort_order")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let host_id: String = r.get(0)?;
        if let Some(&i) = index.get(&host_id) {
            hosts[i].fields.push(HostField {
                key: r.get(1)?,
                value: r.get(2)?,
            });
        }
    }
    Ok(())
}

/// All hosts, including soft-deleted ones (the frontend filters by view).
pub fn list(tx: &Transaction) -> AppResult<Vec<Host>> {
    let mut stmt = tx.prepare(&format!(
        "SELECT {COLUMNS} FROM hosts ORDER BY alias COLLATE NOCASE"
    ))?;
    let mut hosts: Vec<Host> = stmt.query_map([], from_row)?.collect::<Result<_, _>>()?;
    attach_relations(tx, &mut hosts)?;
    Ok(hosts)
}

pub fn get(tx: &Transaction, id: &str) -> AppResult<Host> {
    let host = tx
        .query_row(
            &format!("SELECT {COLUMNS} FROM hosts WHERE id = ?1"),
            [id],
            from_row,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("host {id}")))?;
    let mut hosts = [host];
    attach_relations(tx, &mut hosts)?;
    let [host] = hosts;
    Ok(host)
}

/// Trims an optional text value; empty becomes `None`.
fn clean(v: &Option<String>) -> Option<String> {
    v.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Value that will be written into ssh_config: single line, no leading `-`.
fn check_config_value(label: &str, v: &Option<String>, allow_spaces: bool) -> AppResult<()> {
    if let Some(v) = v {
        let bad = v.starts_with('-')
            || v.chars()
                .any(|c| c.is_control() || (!allow_spaces && c.is_whitespace()));
        if bad {
            return Err(AppError::Validation(format!("invalid {label}: {v:?}")));
        }
    }
    Ok(())
}

/// Normalizes and validates input in place.
fn validate(input: &mut HostInput) -> AppResult<()> {
    input.alias = input.alias.trim().to_string();
    validate_alias(&input.alias)?;

    input.hostname = clean(&input.hostname);
    input.user = clean(&input.user);
    input.identity_file = clean(&input.identity_file);
    input.proxy_jump = clean(&input.proxy_jump);
    input.color = clean(&input.color);
    check_config_value("hostname", &input.hostname, false)?;
    check_config_value("user", &input.user, false)?;
    check_config_value("identity file", &input.identity_file, true)?;
    check_config_value("proxy jump", &input.proxy_jump, false)?;
    if input.port == Some(0) {
        return Err(AppError::Validation("port must be 1–65535".into()));
    }

    validate_options(&mut input.extra_options)?;

    input
        .fields
        .retain(|f| !f.key.trim().is_empty() || !f.value.trim().is_empty());
    for f in &mut input.fields {
        f.key = f.key.trim().to_string();
        if f.key.is_empty() {
            return Err(AppError::Validation(
                "custom field name cannot be empty".into(),
            ));
        }
    }

    validate_user_matches(&mut input.user_matches)?;

    input.description = input.description.trim().to_string();
    Ok(())
}

/// Per-user blocks: a valid, distinct user each; values that stay one config line.
fn validate_user_matches(matches: &mut Vec<UserMatch>) -> AppResult<()> {
    matches.retain(|m| {
        !m.user.trim().is_empty()
            || m.identity_file
                .as_deref()
                .is_some_and(|f| !f.trim().is_empty())
    });
    let mut seen = std::collections::HashSet::new();
    for m in matches.iter_mut() {
        m.user = m.user.trim().to_string();
        crate::security::validate_user(&m.user)
            .map_err(|_| AppError::Validation(format!("invalid user name {:?}", m.user)))?;
        if !seen.insert(m.user.clone()) {
            return Err(AppError::Validation(format!(
                "user {} is listed twice",
                m.user
            )));
        }
        m.identity_file = clean(&m.identity_file);
        check_config_value("identity file", &m.identity_file, true)?;
        validate_options(&mut m.extra_options)?;
    }
    Ok(())
}

/// `Key value` lines without a dedicated field.
fn validate_options(options: &mut Vec<Directive>) -> AppResult<()> {
    options.retain(|d| !d.key.trim().is_empty());
    for d in options {
        d.key = d.key.trim().to_string();
        d.value = d.value.trim().to_string();
        // `Host` and `Match` would start a new block and change the rest of the file.
        if !d.key.chars().all(|c| c.is_ascii_alphanumeric())
            || d.key.eq_ignore_ascii_case("host")
            || d.key.eq_ignore_ascii_case("match")
        {
            return Err(AppError::Validation(format!(
                "invalid option name: {:?}",
                d.key
            )));
        }
        if d.value.chars().any(char::is_control) {
            return Err(AppError::Validation(format!("invalid value for {}", d.key)));
        }
    }
    Ok(())
}

fn map_unique(alias: &str) -> impl FnOnce(rusqlite::Error) -> AppError + '_ {
    move |e| {
        if is_unique_violation(&e) {
            AppError::Conflict(format!("a host named \"{alias}\" already exists"))
        } else {
            AppError::Db(e)
        }
    }
}

fn write_relations(tx: &Transaction, id: &str, input: &HostInput, source: Source) -> AppResult<()> {
    tx.execute("DELETE FROM host_tags WHERE host_id = ?1", [id])?;
    for tag in tags::ensure(tx, &input.tags, source)? {
        tx.execute(
            "INSERT INTO host_tags (host_id, tag_id) VALUES (?1, ?2)",
            params![id, tag.id],
        )?;
    }
    tx.execute("DELETE FROM host_fields WHERE host_id = ?1", [id])?;
    for (i, f) in input.fields.iter().enumerate() {
        tx.execute(
            "INSERT INTO host_fields (host_id, key, value, sort_order) VALUES (?1, ?2, ?3, ?4)",
            params![id, f.key, f.value, i as i64],
        )?;
    }
    Ok(())
}

pub fn create(tx: &Transaction, mut input: HostInput, source: Source) -> AppResult<Host> {
    validate(&mut input)?;
    let id = new_id();
    let ts = now();
    tx.execute(
        "INSERT INTO hosts (id, alias, hostname, user, port, identity_file, proxy_jump, extra_options,
            group_id, description, notes, color, favorite, sync_state, config_dirty, created_at, updated_at,
            user_matches)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'db_only', 0, ?14, ?14, ?15)",
        params![
            id,
            input.alias,
            input.hostname,
            input.user,
            input.port,
            input.identity_file,
            input.proxy_jump,
            serde_json::to_string(&input.extra_options)?,
            input.group_id,
            input.description,
            input.notes,
            input.color,
            input.favorite,
            ts,
            serde_json::to_string(&input.user_matches)?
        ],
    )
    .map_err(map_unique(&input.alias))?;
    write_relations(tx, &id, &input, source)?;
    let host = get(tx, &id)?;
    history::record(
        tx,
        source,
        EntityType::Host,
        &id,
        Action::Create,
        None::<&Host>,
        Some(&host),
    )?;
    Ok(host)
}

impl Host {
    pub fn connection(&self) -> Connection {
        Connection {
            alias: self.alias.clone(),
            hostname: self.hostname.clone(),
            user: self.user.clone(),
            port: self.port,
            identity_file: self.identity_file.clone(),
            proxy_jump: self.proxy_jump.clone(),
            extra_options: self.extra_options.clone(),
            user_matches: self.user_matches.clone(),
        }
    }
}

impl HostInput {
    pub fn connection(&self) -> Connection {
        Connection {
            alias: self.alias.clone(),
            hostname: self.hostname.clone(),
            user: self.user.clone(),
            port: self.port,
            identity_file: self.identity_file.clone(),
            proxy_jump: self.proxy_jump.clone(),
            extra_options: self.extra_options.clone(),
            user_matches: self.user_matches.clone(),
        }
    }
}

pub fn update(tx: &Transaction, id: &str, mut input: HostInput, source: Source) -> AppResult<Host> {
    validate(&mut input)?;
    let before = get(tx, id)?;
    if before.deleted_at.is_some() {
        return Err(AppError::Conflict(
            "restore the host from trash before editing it".into(),
        ));
    }
    // Pending config edits = the DB connection differs from what the config says. Hosts that
    // are not in the config have nothing to be pending against.
    let dirty = before
        .config_snapshot
        .as_ref()
        .is_some_and(|s| *s != input.connection());
    tx.execute(
        "UPDATE hosts SET alias = ?2, hostname = ?3, user = ?4, port = ?5, identity_file = ?6,
            proxy_jump = ?7, extra_options = ?8, group_id = ?9, description = ?10, notes = ?11,
            color = ?12, favorite = ?13, config_dirty = ?14, updated_at = ?15, user_matches = ?16
         WHERE id = ?1",
        params![
            id,
            input.alias,
            input.hostname,
            input.user,
            input.port,
            input.identity_file,
            input.proxy_jump,
            serde_json::to_string(&input.extra_options)?,
            input.group_id,
            input.description,
            input.notes,
            input.color,
            input.favorite,
            dirty,
            now(),
            serde_json::to_string(&input.user_matches)?
        ],
    )
    .map_err(map_unique(&input.alias))?;
    write_relations(tx, id, &input, source)?;
    let after = get(tx, id)?;
    if after != before {
        history::record(
            tx,
            source,
            EntityType::Host,
            id,
            Action::Update,
            Some(&before),
            Some(&after),
        )?;
    }
    Ok(after)
}

/// Small metadata updates (favorite toggle, drag to group) without a full input round-trip.
pub fn patch_meta(
    tx: &Transaction,
    ids: &[String],
    favorite: Option<bool>,
    group_id: Option<Option<String>>,
    source: Source,
) -> AppResult<()> {
    for id in ids {
        let before = get(tx, id)?;
        tx.execute(
            "UPDATE hosts SET favorite = COALESCE(?2, favorite),
                group_id = CASE WHEN ?3 THEN ?4 ELSE group_id END, updated_at = ?5
             WHERE id = ?1",
            params![
                id,
                favorite,
                group_id.is_some(),
                group_id.clone().flatten(),
                now()
            ],
        )?;
        let after = get(tx, id)?;
        if after != before {
            history::record(
                tx,
                source,
                EntityType::Host,
                id,
                Action::Update,
                Some(&before),
                Some(&after),
            )?;
        }
    }
    Ok(())
}

/// Moves hosts to trash. Notes and history are kept until purged.
pub fn soft_delete(tx: &Transaction, ids: &[String], source: Source) -> AppResult<()> {
    for id in ids {
        let before = get(tx, id)?;
        if before.deleted_at.is_some() {
            continue;
        }
        tx.execute(
            "UPDATE hosts SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1",
            params![id, now()],
        )?;
        let after = get(tx, id)?;
        history::record(
            tx,
            source,
            EntityType::Host,
            id,
            Action::Delete,
            Some(&before),
            Some(&after),
        )?;
    }
    Ok(())
}

pub fn restore(tx: &Transaction, id: &str, source: Source) -> AppResult<Host> {
    let before = get(tx, id)?;
    tx.execute(
        "UPDATE hosts SET deleted_at = NULL, updated_at = ?2 WHERE id = ?1",
        params![id, now()],
    )
    .map_err(map_unique(&before.alias))?;
    let after = get(tx, id)?;
    history::record(
        tx,
        source,
        EntityType::Host,
        id,
        Action::Restore,
        Some(&before),
        Some(&after),
    )?;
    Ok(after)
}

/// Permanently deletes a host that is already in trash.
pub fn purge(tx: &Transaction, id: &str, source: Source) -> AppResult<()> {
    let before = get(tx, id)?;
    if before.deleted_at.is_none() {
        return Err(AppError::Conflict(
            "only hosts in trash can be deleted permanently".into(),
        ));
    }
    tx.execute("DELETE FROM hosts WHERE id = ?1", [id])?;
    history::record(
        tx,
        source,
        EntityType::Host,
        id,
        Action::Purge,
        Some(&before),
        None::<&Host>,
    )
}

/// Is this trashed host's block still in ~/.ssh/config? Purging it would only make the next
/// scan import it again, so housekeeping leaves such hosts alone.
pub fn still_in_config(h: &Host) -> bool {
    h.config_snapshot.is_some() && h.sync_state != "removed_from_config"
}

/// Permanently deletes trashed hosts (all of them, or those trashed before `older_than`),
/// except ones still in the config. Returns the aliases purged and how many were kept.
pub fn purge_trash(
    tx: &Transaction,
    older_than: Option<&str>,
    source: Source,
) -> AppResult<(Vec<String>, usize)> {
    let mut purged = Vec::new();
    let mut kept = 0;
    for h in list(tx)?.into_iter().filter(|h| h.deleted_at.is_some()) {
        let old_enough = older_than.is_none_or(|cutoff| h.deleted_at.as_deref() < Some(cutoff));
        if !old_enough {
            continue;
        }
        if still_in_config(&h) {
            kept += 1;
            continue;
        }
        purge(tx, &h.id, source)?;
        purged.push(h.alias);
    }
    Ok((purged, kept))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn input(alias: &str) -> HostInput {
        HostInput {
            alias: alias.into(),
            hostname: Some(" 10.0.0.5 ".into()),
            user: Some("deploy".into()),
            port: Some(22),
            tags: vec!["prod".into(), "PROD".into(), "db".into()],
            fields: vec![HostField {
                key: "Owner".into(),
                value: "ops".into(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn create_normalizes_and_dedupes_tags() {
        let db = Db::open_in_memory().unwrap();
        let h = db
            .tx(|tx| create(tx, input("prod-db"), Source::User))
            .unwrap();
        assert_eq!(h.hostname.as_deref(), Some("10.0.0.5"));
        assert_eq!(
            h.tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["db", "prod"]
        );
        assert_eq!(h.fields.len(), 1);
        assert_eq!(h.sync_state, "db_only");
    }

    #[test]
    fn alias_unique_among_live_hosts_only() {
        let db = Db::open_in_memory().unwrap();
        db.tx(|tx| {
            let a = create(tx, input("web"), Source::User)?;
            assert!(matches!(
                create(tx, input("web"), Source::User),
                Err(AppError::Conflict(_))
            ));
            soft_delete(tx, std::slice::from_ref(&a.id), Source::User)?;
            create(tx, input("web"), Source::User)?;
            // Restoring would now clash with the new live host.
            assert!(matches!(
                restore(tx, &a.id, Source::User),
                Err(AppError::Conflict(_))
            ));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn rejects_injection_in_config_values() {
        let db = Db::open_in_memory().unwrap();
        let mut bad = input("x");
        bad.hostname = Some("-oProxyCommand=calc".into());
        assert!(db.tx(|tx| create(tx, bad, Source::User)).is_err());
        let mut bad = input("y");
        bad.user = Some("a\nHost evil".into());
        assert!(db.tx(|tx| create(tx, bad, Source::User)).is_err());
    }

    #[test]
    fn writes_audit_log() {
        let db = Db::open_in_memory().unwrap();
        db.tx(|tx| {
            let h = create(tx, input("a"), Source::User)?;
            let mut i = input("a");
            i.description = "primary".into();
            update(tx, &h.id, i, Source::User)?;
            soft_delete(tx, std::slice::from_ref(&h.id), Source::User)?;
            purge(tx, &h.id, Source::User)?;
            let log = history::for_entity(tx, EntityType::Host, &h.id, 10)?;
            let actions: Vec<_> = log.iter().map(|e| e.action.as_str()).collect();
            assert_eq!(actions, ["purge", "delete", "update", "create"]);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn config_dirty_tracks_difference_from_snapshot() {
        let db = Db::open_in_memory().unwrap();
        db.tx(|tx| {
            let h = create(tx, input("a"), Source::User)?;
            let mut i = input("a");
            i.port = Some(2222);
            assert!(!update(tx, &h.id, i.clone(), Source::User)?.config_dirty);
            // Pretend the config contains the host as it is now.
            let snapshot = serde_json::to_string(&get(tx, &h.id)?.connection())?;
            tx.execute(
                "UPDATE hosts SET sync_state = 'synced', config_snapshot = ?2 WHERE id = ?1",
                params![h.id, snapshot],
            )?;
            i.port = Some(2223);
            assert!(update(tx, &h.id, i.clone(), Source::User)?.config_dirty);
            // Editing back to the config's value clears the pending flag.
            i.port = Some(2222);
            assert!(!update(tx, &h.id, i, Source::User)?.config_dirty);
            Ok(())
        })
        .unwrap();
    }
}
