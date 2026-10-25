-- Per-user settings of a host: its `Match originalhost <alias> user <name>` blocks in ssh_config.
-- JSON [{user, identityFile, extraOptions}], config order. Filled by the next config scan.
ALTER TABLE hosts ADD COLUMN user_matches TEXT NOT NULL DEFAULT '[]';
