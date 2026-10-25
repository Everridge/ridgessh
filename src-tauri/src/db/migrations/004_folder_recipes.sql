-- Saved folder transfers (M11): a name and the transfer's settings as JSON (folder::FolderSpec).
CREATE TABLE folder_recipes (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    spec_json      TEXT NOT NULL,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    last_run_at    TEXT,
    -- 0 = finished, other = failed, NULL with last_run_at = cancelled or still running.
    last_exit_code INTEGER
);
CREATE UNIQUE INDEX folder_recipes_name ON folder_recipes (name COLLATE NOCASE);
