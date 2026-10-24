-- What ~/.ssh/config said about this host at the last scan (JSON Connection), and which
-- file it came from (NULL = the main config file).
ALTER TABLE hosts ADD COLUMN config_snapshot TEXT;
ALTER TABLE hosts ADD COLUMN config_source TEXT;
