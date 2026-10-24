-- Backups are self-describing files in the backup folder (see src/backup); a table would
-- go stale whenever an older database is restored.
DROP TABLE backups;
