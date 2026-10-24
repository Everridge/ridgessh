# RidgeSSH — Implementation Plan

A lightweight desktop SSH connection manager. Windows first, macOS second.

## 1. Goals / non-goals

**Goals**

- Manage SSH hosts: groups (nested), add/edit/delete, description, notes, custom fields, tags, favorites.
- `~/.ssh/config` stays the source of truth for _connection_ settings; SQLite holds _metadata_ (groups, notes, history).
- Startup sync: scan the config, add unknown hosts to DB, flag DB hosts that disappeared from the config.
- Config file is written **only** after a diff preview + typing a confirmation word.
- DB changes are less restricted (normal confirm dialog, soft delete, undo via history).
- Embedded terminal (tabs), SCP file transfer, upload-and-run `.sh` scripts.
- Encrypted (password-protected) backups, export and import; selectable backup folder; auto-backup before changes.
- Full history: audit log of every change + log of sessions/transfers/script runs.

**Non-goals (v1)**

- Not a full RDM replacement: no RDP/VNC, no team/cloud sync.
- No custom SSH implementation — we drive the system OpenSSH (`ssh.exe`, `scp.exe`) so ProxyJump, Match, agent, keys all behave exactly as in a normal terminal.

## 2. Stack

| Layer         | Choice                                                                                | Why                                                                          |
| ------------- | ------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| Shell         | **Tauri 2**                                                                           | Small binary, Rust backend, Win + macOS                                      |
| UI            | React 19 + TypeScript + Vite                                                          | shadcn/ui is React-first                                                     |
| Styling       | Tailwind CSS v4 + **shadcn/ui**                                                       | as requested                                                                 |
| UI state      | TanStack Query (backend data), Zustand (UI state), TanStack Table (host list)         |                                                                              |
| Terminal      | **xterm.js** (+ fit, search, web-links addons)                                        |                                                                              |
| PTY           | `portable-pty` (ConPTY on Windows)                                                    | spawns system `ssh.exe`                                                      |
| DB            | SQLite via `rusqlite` (bundled) + `rusqlite_migration`                                | all DB access in Rust, never from JS                                         |
| Crypto        | `age` crate, passphrase (scrypt) mode                                                 | audited format; files can be decrypted with the `age` CLI if the app is gone |
| Secrets       | `keyring` crate (Windows Credential Manager / macOS Keychain)                         | stores backup password so auto-backup works unattended                       |
| Archive       | `zip`                                                                                 | backup bundle before encryption                                              |
| Diff          | `similar`                                                                             | config diff preview                                                          |
| Tauri plugins | dialog, fs (scoped), store not needed, updater (later), single-instance, window-state |                                                                              |

All business logic lives in Rust; the frontend only calls typed `#[tauri::command]`s and listens to events. The Tauri `shell` plugin is **not** exposed to the webview.

## 3. Architecture

```
src-tauri/src/
  main.rs / lib.rs         app setup, state, command registration
  db/                      connection pool, migrations, repositories
  ssh_config/
    lexer.rs, cst.rs       lossless parser (keeps comments, blank lines, order, CRLF)
    model.rs               HostBlock, MatchBlock, Include, Directive
    writer.rs              apply edits to CST → text; atomic write
  sync/                    config ↔ DB reconciliation, rename detection
  backup/                  bundle, encrypt, restore, retention, auto-backup triggers
  transfer/                export/import (same format as backup, selective)
  terminal/                PTY sessions, resize, stream to frontend via Channel
  scp/                     upload/download jobs, progress
  scripts/                 script library, remote execution
  history/                 audit log writer + queries, restore-from-history
  settings/                key/value settings
  security/                arg validation, path checks
src/                       React app
  components/ui/           shadcn
  features/hosts, groups, terminal, transfers, scripts, sync, backups, history, settings
  lib/ipc.ts               typed wrappers around invoke()
```

Process spawning rules: always `Command::new(ssh_path).args([...])` with an argument vector — never a shell string. Host aliases are validated (no leading `-`, no whitespace/control chars) and passed after `--` where supported.

## 4. Data model (SQLite)

```sql
groups(id, parent_id NULL, name, color, icon, sort_order, created_at, updated_at)

hosts(
  id TEXT PK (uuid), alias TEXT UNIQUE,          -- the `Host` name in config
  hostname, user, port, identity_file, proxy_jump,
  extra_options JSON,                            -- all other directives, ordered
  group_id NULL, description, notes (markdown), color, favorite,
  sync_state TEXT,     -- synced | config_only_new | removed_from_config | db_only | conflict
  config_fingerprint,  -- hash of the block as last seen in config
  last_seen_in_config_at, removed_from_config_at,
  last_connected_at, deleted_at NULL,            -- soft delete
  created_at, updated_at)

host_fields(host_id, key, value, sort_order)     -- "additional info" (custom fields)
tags(id, name, color);  host_tags(host_id, tag_id)

scripts(id, name, description, content, interpreter DEFAULT 'bash',
        default_args, run_mode, created_at, updated_at)

audit_log(id, ts, batch_id, source,             -- user | sync | import | restore
          entity_type, entity_id, action,        -- create | update | delete | restore
          before_json, after_json)

activity(id, host_id, kind,                      -- terminal | scp_up | scp_down | script
         started_at, ended_at, exit_code, details_json)

backups(id, file_path, created_at, reason, size, sha256, contents_json)
settings(key PK, value_json)
```

Passwords are **never** stored in SQLite. If password auth is ever supported, the secret goes to the OS keychain keyed by host id.

## 5. SSH config handling

Your current config has 52 `Host` blocks, 7 `Match` blocks, ProxyJump and LocalForward — so the parser must be lossless.

**Parser**

- Concrete syntax tree: every line kept (comments, blank lines, indentation, `key value` vs `key=value`, quoting, line endings).
- Block types: global preamble, `Host <patterns>`, `Match ...`, `Include` (followed recursively for reading; included files are read-only in v1 and hosts from them are labelled with their source file).
- Only `Host` blocks with a single non-wildcard pattern become managed hosts. Wildcard blocks (`Host *`, `Host *.corp`) and `Match` blocks are shown read-only as "rules" and are never rewritten.

**Startup sync** (also runnable manually)

1. Parse config → set of managed blocks keyed by alias.
2. For each alias:
   - in config, not in DB → insert, `sync_state = config_only_new` (badge "new from config").
   - in both, fingerprint unchanged → `synced`.
   - in both, config changed → update DB connection fields from config (config wins), write audit entry with source `sync`.
   - DB has unsaved edits _and_ config changed → `conflict`, resolve in UI.
3. DB hosts whose alias is missing from config → `removed_from_config` (kept with notes/history, never auto-deleted). User can: restore into config, keep as DB-only, or delete.
4. Rename heuristic: a "removed" + a "new" host with the same hostname/user/port → suggest "was this renamed?" to carry notes/group/history over.
5. Show a non-blocking sync summary ("3 new, 1 removed, 0 conflicts") with a review screen.

DB-side edits to connection fields mark the host as having **pending config changes**; nothing is written until the user applies.

**Writing the config (guarded)**

1. Collect pending changes → produce new text from the CST (only touched blocks change).
2. Show a unified diff (`similar`) per host + whole file.
3. User types the confirmation word (default `CONFIRM`, configurable in settings; button disabled until it matches exactly).
4. Re-read the file and compare hash with the one we parsed; if changed externally → abort and re-sync.
5. Auto-backup (config + DB).
6. Atomic write: temp file in same dir → flush → replace. Preserve original line endings and encoding.
7. **Windows ACL**: OpenSSH for Windows refuses configs with loose permissions. Copy the original file's ACL onto the new file (or write in place after backup); on macOS keep mode `0600`.
8. Audit entry + mark hosts `synced`.

## 6. Terminal

- Each tab = one PTY running `ssh.exe <alias>` (so it uses the real config, agent, ProxyJump).
- Output streamed to the frontend via a Tauri `Channel<Vec<u8>>`; input/resize via commands.
- Features: tabs, split view (later), search, copy-on-select option, font/theme settings, reconnect button, "Open in Windows Terminal" alternative.
- Connection start/end recorded in `activity`, updates `last_connected_at`.
- Quick connect palette (Ctrl+K) with fuzzy search across alias, hostname, group, tags, description.

## 7. SCP transfers

- Upload: pick local files/folders (drag & drop onto host) → remote path (remembered per host).
- Download: enter remote path → local folder.
- Runs `scp.exe` (with `-r` for directories) inside a PTY so progress output can be parsed into a progress bar; transfer queue panel with cancel.
- Later (v2): SFTP browser for remote directory listing (`sftp -b` or `russh-sftp`).

## 8. Remote scripts

- Script library in DB (name, description, content, default args) + "run a local .sh file".
- Two run modes:
  - **Stream** (default): `ssh <alias> bash -s -- <args>` with the script piped to stdin — nothing left on the server.
  - **Copy & run**: `scp` to `/tmp/ridgessh-<uuid>.sh`, then `ssh <alias> 'chmod +x … && … ; rm -f …'` — for scripts that need a real file / sudo tty.
- Output shown in a terminal tab; exit code + duration logged to `activity`.
- Run on one host or a multi-selection (sequential first, parallel later). Confirmation dialog listing target hosts before running.
- CRLF → LF normalization before sending (common Windows pitfall).

## 9. Backups

**Bundle** (`.rsbak`): zip containing

- `manifest.json` (app version, schema version, created_at, reason, host count, sha256 of each entry)
- `db.sqlite` (consistent snapshot via `VACUUM INTO`)
- `ssh_config` (+ included files)
- optionally referenced key files (off by default, explicit warning)

then encrypted with `age` passphrase mode.

**Password**: set on first run (or first backup); stored in the OS keychain so auto-backups don't prompt. Changing it doesn't re-encrypt old backups (they keep the old password — UI says so). Option "don't remember password" disables auto-backup with a visible warning.

**Folder**: chosen in settings (default `%APPDATA%\RidgeSSH\backups`); validated as writable; can point to OneDrive/Dropbox folder.

**Auto-backup triggers**

- Always: before writing ssh config, before import, before restore, before bulk delete, before applying sync results that change data.
- Ordinary single DB edits: covered by audit log + at most one auto-backup per N minutes (configurable, default 30).
- Retention: keep last N (default 30) and/or N days; manual backups are never pruned automatically.

**Restore**: pick a backup → enter password → preview (what changes: hosts, groups, config diff) → choose DB only / config only / both → config part still requires the confirmation word.

**As built (M5)**: the backup folder is the list of backups. File names carry time, manual/auto and reason (`ridgessh-<UTC>-auto-config-write.rsbak`), and there is no `backups` table, because it would go stale whenever an older database is restored. Plain copies of replaced configs go to `<app data>/config-history`, never to the (possibly cloud-synced) backup folder. Key files are not included yet.

## 10. Export / import

- Same encrypted `age` format, file extension `.rsexport`, with its own password (asked each time, not stored).
- Export scope: everything, or selected groups/hosts; optional scripts; optional key files (warning).
- Import: decrypt → preview table → per-item strategy: skip / overwrite / import as copy; group mapping. Auto-backup first. Imported connection data lands in DB as pending config changes → user applies via the guarded write.
- Extra: import a plain `ssh_config` file from another machine; export selected hosts as a plain config snippet (unencrypted, explicit).

**As built (M8)**: an `.rsexport` is an age-encrypted (scrypt) JSON document rather than a zip, so `age -d` opens it as readable text. Hosts are matched by alias and scripts by name. Each item is _new_, _already here_ or _differs_ (with the differing fields listed). New items can be imported or skipped; existing ones can be skipped, overwritten (the host keeps its local group) or imported as a copy (`alias-2`). Imported groups are matched by name under the chosen parent and created only when a host needs them. The import is one history batch, so it can be undone, and it runs after a strict auto-backup. Key files are opt-in on export. On import they are written only inside `~/.ssh` and only where no file exists. The plain config export refuses to write over the live config or its includes.

## 11. History

- **Audit log**: every create/update/delete with before/after JSON, grouped by `batch_id` (e.g., one import = one batch). Per-host timeline in the host details panel, global history view with filters.
- **Restore from history**: revert a host to a previous version, undelete soft-deleted hosts (trash view, purge after N days).
- **Activity log**: connections, transfers, script runs with exit codes.

## 12. UI layout

```
┌───────────────┬──────────────────────────────┬──────────────────┐
│ Groups tree   │ Host list (table / cards)    │ Host details     │
│ ★ Favorites   │ search · filter by tag/state │ connection       │
│ Recent        │ badges: new / removed /      │ description      │
│ Removed       │ pending / conflict           │ notes, fields    │
│ Trash         │                              │ history timeline │
├───────────────┴──────────────────────────────┴──────────────────┤
│ Terminal tabs / transfers / script output (resizable panel)     │
└─────────────────────────────────────────────────────────────────┘
Top bar: Ctrl+K quick connect · Sync status · "Apply N config changes" · Backup
```

shadcn pieces: Sidebar, Resizable, DataTable, Dialog/Sheet, Command (Ctrl+K), Tabs, Form + zod, Sonner toasts, Badge, AlertDialog (with confirmation-word input). Light/dark theme.

## 13. Security checklist

- Webview has no shell/fs-wide access; Tauri capabilities scoped to required commands only; strict CSP.
- Argument-vector process spawning only; alias/path validation.
- Backup/export encrypted (age/scrypt); passwords only in OS keychain, never in DB/logs.
- Warn when notes look like they contain secrets (optional, later); consider SQLCipher for the DB as a v2 option.
- Config writes: external-change detection, backup, atomic replace, ACL preservation.

## 14. Milestones

| #   | Milestone            | Deliverable                                                                                                      |
| --- | -------------------- | ---------------------------------------------------------------------------------------------------------------- |
| M0  | Scaffold             | Tauri 2 + React + Vite + Tailwind v4 + shadcn, Rust module skeleton, lint/format, Vitest + `cargo test`          |
| M1  | DB & CRUD            | migrations, hosts/groups/tags/fields repos, 3-pane UI, soft delete, audit log writes                             |
| M2  | Config read & sync   | lossless parser (+ round-trip tests on real-world configs), startup sync, review screen, rename suggestions      |
| M3  | Terminal             | PTY + xterm tabs, quick connect, activity logging                                                                |
| M4  | Guarded config write | pending changes, diff view, confirmation word, ACL-preserving atomic write                                       |
| M5  | Backups              | settings (folder, retention, confirmation word), password + keychain, auto-backup triggers, restore with preview |
| M6  | History UI           | timelines, revert, trash                                                                                         |
| M7  | SCP & scripts        | transfer queue with progress, script library, stream / copy-and-run modes                                        |
| M8  | Export / import      | selective encrypted export, import preview + merge strategies, plain config import                               |
| M9  | Release (Windows)    | NSIS installer, single-instance, window state, updater, code signing                                             |
| M10 | macOS                | keychain, `0600` perms, `/usr/bin/ssh`, notarization                                                             |

**As built (M9)**: a per-user NSIS installer (no admin rights; downloads WebView2 if it is missing). The updater plugin reads `latest.json` from the latest published GitHub release and checks its minisign signature. The public key is in `tauri.conf.json`; the private key and its password are kept outside the repo and stored as CI secrets. The app checks once on startup (there is a setting to turn this off) and again whenever the user clicks "Check now" in Settings → About. The update dialog shows the release notes and warns when terminal sessions are open. The NSIS installer then runs in passive mode and the app restarts. Before a newer schema is applied, the database is copied to `db.v<N>.bak`, and a database from a newer version is refused instead of being opened. Releases: `pnpm version:bump X.Y.Z` → `CHANGELOG.md` section → tag `vX.Y.Z` → the `release.yml` workflow builds the installer and creates a draft release. The draft becomes visible to the updater once it is published. Authenticode signing uses Azure Trusted Signing; it is switched on by repository variables and secrets, and builds are unsigned until those are set.

**As built (M10)**:

- The macOS build uses `/usr/bin/ssh` and `/usr/bin/scp` and stores the backup password in the Keychain.
- New config and key files are created with mode `0600`, and replaced files keep their mode.
- A symlinked `~/.ssh/config` is written through the link, so the link survives.
- Apps opened from Finder start without `LANG` and without Homebrew on `PATH`, so terminal sessions get both.
- "Open in external terminal" writes a temporary `.command` file that deletes itself, then opens it in Terminal or whichever app handles `.command` files.
- Shortcuts use ⌘ (⌘K for quick connect). ⌘C and ⌘V use the standard Edit menu, and Ctrl+C always reaches the shell.
- UI text names Finder and the Keychain instead of Explorer and Credential Manager.
- Bundles are a universal `.app` and a `.dmg` for macOS 11 and later. The updater uses `.app.tar.gz`.
- CI runs every check on macOS and builds the app bundle there.
- The release workflow signs and notarizes when Apple Developer secrets are set. Without them the app is only ad-hoc signed.

Critical path: **M2 parser** (must be lossless — the one place a bug can damage the user's real config) and **M4 write path**. Both get heavy unit + golden-file tests before any write is enabled.

## 15. Decisions taken (change if you disagree)

1. System OpenSSH binaries instead of an embedded SSH library — full config compatibility.
2. Config is the source of truth for connection fields; DB for everything else.
3. Wildcard `Host` and `Match` blocks are read-only in v1.
4. Confirmation word: one configurable word (default `CONFIRM`).
5. Backup password remembered in the OS keychain; export password asked every time.
6. Private keys are excluded from backups/exports unless explicitly enabled.
