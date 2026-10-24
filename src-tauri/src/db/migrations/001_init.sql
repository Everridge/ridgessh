-- Initial schema. Timestamps are RFC 3339 UTC strings.

CREATE TABLE groups (
    id          TEXT PRIMARY KEY,
    parent_id   TEXT REFERENCES groups(id) ON DELETE SET NULL,
    name        TEXT NOT NULL,
    color       TEXT,
    icon        TEXT,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX idx_groups_parent ON groups(parent_id);

CREATE TABLE hosts (
    id                      TEXT PRIMARY KEY,
    alias                   TEXT NOT NULL,
    hostname                TEXT,
    user                    TEXT,
    port                    INTEGER,
    identity_file           TEXT,
    proxy_jump              TEXT,
    extra_options           TEXT NOT NULL DEFAULT '[]',   -- JSON [{key, value}], config order
    group_id                TEXT REFERENCES groups(id) ON DELETE SET NULL,
    description             TEXT NOT NULL DEFAULT '',
    notes                   TEXT NOT NULL DEFAULT '',
    color                   TEXT,
    favorite                INTEGER NOT NULL DEFAULT 0,
    -- synced | config_only_new | removed_from_config | db_only | conflict
    sync_state              TEXT NOT NULL DEFAULT 'db_only',
    -- connection fields edited in the DB but not yet written to ~/.ssh/config
    config_dirty            INTEGER NOT NULL DEFAULT 0,
    config_fingerprint      TEXT,
    last_seen_in_config_at  TEXT,
    removed_from_config_at  TEXT,
    last_connected_at       TEXT,
    deleted_at              TEXT,
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL
);
-- Aliases are unique among live hosts; soft-deleted hosts may share an alias.
CREATE UNIQUE INDEX idx_hosts_alias_live ON hosts(alias) WHERE deleted_at IS NULL;
CREATE INDEX idx_hosts_group ON hosts(group_id);

CREATE TABLE host_fields (
    host_id     TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    sort_order  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_host_fields_host ON host_fields(host_id);

CREATE TABLE tags (
    id      TEXT PRIMARY KEY,
    name    TEXT NOT NULL UNIQUE COLLATE NOCASE,
    color   TEXT
);

CREATE TABLE host_tags (
    host_id TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    tag_id  TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (host_id, tag_id)
);

CREATE TABLE scripts (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    description   TEXT NOT NULL DEFAULT '',
    content       TEXT NOT NULL,
    interpreter   TEXT NOT NULL DEFAULT 'bash',
    default_args  TEXT NOT NULL DEFAULT '',
    run_mode      TEXT NOT NULL DEFAULT 'stream',   -- stream | copy
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE audit_log (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    ts           TEXT NOT NULL,
    batch_id     TEXT,
    source       TEXT NOT NULL,   -- user | sync | import | restore
    entity_type  TEXT NOT NULL,   -- host | group | tag | script | settings
    entity_id    TEXT NOT NULL,
    action       TEXT NOT NULL,   -- create | update | delete | restore | purge
    before_json  TEXT,
    after_json   TEXT
);
CREATE INDEX idx_audit_entity ON audit_log(entity_type, entity_id);
CREATE INDEX idx_audit_ts ON audit_log(ts);

CREATE TABLE activity (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    host_id       TEXT REFERENCES hosts(id) ON DELETE SET NULL,
    kind          TEXT NOT NULL,   -- terminal | scp_up | scp_down | script
    started_at    TEXT NOT NULL,
    ended_at      TEXT,
    exit_code     INTEGER,
    details_json  TEXT
);
CREATE INDEX idx_activity_host ON activity(host_id);

CREATE TABLE backups (
    id             TEXT PRIMARY KEY,
    file_path      TEXT NOT NULL,
    created_at     TEXT NOT NULL,
    reason         TEXT NOT NULL,
    size           INTEGER NOT NULL,
    sha256         TEXT NOT NULL,
    contents_json  TEXT NOT NULL
);

CREATE TABLE settings (
    key         TEXT PRIMARY KEY,
    value_json  TEXT NOT NULL
);
