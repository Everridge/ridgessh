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

**Per-user `Match` blocks (as built)**: `Match originalhost <alias> user <name>` blocks (criteria in either order) belong to the host `<alias>`. They are part of its connection as `user_matches` (`hosts.user_matches`, migration 005: user, the first `IdentityFile`, other lines). They sync, conflict, show as pending and go into history, export and revert like the other connection fields. Users are edited on the host form's **Users** tab, and a regular host gets its first one there. The writer rewrites only the lines that change, renames a block's header when the host is renamed (keeping the criteria order), deletes the blocks of removed users or a removed host, and adds new blocks after the host's last one (or right after its `Host` block). The verifier reads the result back and checks every host's users. A host whose `Match` blocks changed outside since the last scan, or are in an included file, is blocked until a re-scan. Hosts scanned before migration 005 take their blocks over on the next scan without a conflict, and their pending edits are kept. With pending edits, connecting as `user@alias` passes that user's options with `-o`, before the host's own. Other `Match` blocks stay read-only rules.

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

## 8a. Folder transfers (M11)

Moving whole folders: remote → this PC, this PC → remote, and remote → remote via this PC. A transfer is a **recipe**: a source, a destination and an ordered list of steps, each of which can be switched on or off. It runs in a terminal tab like scp does, so password and host-key prompts still work, and a strip above the output shows the status of each step.

| #   | Step              | Where                | Default                                                                                                                                          |
| --- | ----------------- | -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1   | Run script before | source               | off (a library script or a command)                                                                                                              |
| 2   | Check             | source + destination | on: the folder exists, its size (`du`), free space (`df`, local disk), and whether `zip`/`unzip` or the `python3` fallback is available          |
| 3   | Pack              | source               | on: `zip -r -y` (keeps permissions and symlinks) into `/tmp/ridgessh-<id>.zip`, paths relative to the folder's parent; excludes; optional `sudo` |
| 4   | Download          | source → PC          | on: scp, then compare SHA-256 checksums of both copies                                                                                           |
| 5   | Unpack locally    | PC                   | off: `tar -xf` (bsdtar reads zip on Windows 10+ and macOS)                                                                                       |
| 6   | Upload            | PC → destination     | on when the destination is remote                                                                                                                |
| 7   | Unpack            | destination          | on: `unzip -o`. If the target exists, rename it to `name.bak-<date>` first (default), merge, or stop                                             |
| 8   | Set owner         | destination          | off: `chown -R user:group` with `sudo` (zip does not keep owners)                                                                                |
| 9   | Run script after  | destination          | off                                                                                                                                              |
| 10  | Clean up          | everywhere           | on: deletes this run's temporary archives (only `/tmp/ridgessh-*.zip` it created); the local copy is kept when asked                             |

- **Zip, not tar**: opens natively on Windows, macOS and Linux. Missing `zip`/`unzip` on a server is found by Check; the fallback is `python3 -m zipfile`, or installing the package after asking.
- **Execution**: the backend runs the steps in order, each as its own ssh/scp process feeding the same tab. A step reports results (sizes, checksums) as marker lines in its output, so prompts keep working. A failed step stops the run; **Retry** continues from the failed step, keeping what already succeeded. Each run is recorded in the activity log.
- **Safety**: all paths are quoted before they go into a remote command. Temporary files exist only under `/tmp/ridgessh-*`. A preview lists each step's exact command, and overwriting asks for confirmation. The ssh config is never touched.
- Windows OpenSSH cannot reuse connections (no `ControlMaster`), so password hosts are asked once per step. Key-based hosts are not affected.

Phases:

1. **Remote → PC**: check, pack, download with checksum, optional unpack locally, clean up.
2. **PC → remote and remote → PC → remote**: upload, unpack with backup of the existing folder, set owner.
3. **Scripts before and after, `sudo`, excludes, saved recipes** (a Recipes list with run history; steps editable per run), and a remote folder browser.
4. **Optional direct streaming between servers** (`ssh A "zip -" | ssh B "…"` through the app): no local disk use, but no resume.

**As built (M11 phase 1)**: A host or user row's context menu has "Folder as zip" (download, upload, or copy to another server), and the details panel has a folder button. It runs Check → Zip on server → Download → (Unpack locally) → Clean up in one tab. The backend runner (`folder/runner.rs`) starts each step as its own PTY process behind a terminal route. A route is a stable session id that sends input to whichever process is current and holds input that arrives between processes. A step strip above the tab shows each step's status. Remote scripts get their arguments as quoted positional parameters and report problems through exit codes (10: folder not found, 11: no packer, 12: no space, 13: no access, 18: some files unreadable, which is a warning). The archive in `/tmp` is created with `umask 077`. Its checksum, size and folder name travel in a `.sum` file that is downloaded first, and the local disk's free space is checked before the archive itself is downloaded. The archive is downloaded as `.part` and renamed only after the size and SHA-256 match. Unpacking uses the system `tar` (bsdtar), after moving an existing folder to `name.bak-<date>`. A failed or cancelled run is kept in memory, and **Retry** continues from the step that did not finish. Clean-up deletes only `/tmp/ridgessh-<12 hex>.zip` and its `.sum`.

**As built (M11 phase 2)**:

- The spec is a source and a destination, each a server (host, optional user, path) or this PC.
- On this PC, folders are zipped in Rust with the `zip` crate. No external tool is needed on Windows. Permissions, symlinks (on Unix) and modification times are kept, entries are sorted, and unreadable entries are a warning.
- Check destination (unzip or python3, write access to the nearest existing parent, free space, existing folder) runs before anything is packed, so "stop" fails early.
- An upload goes to `/tmp/ridgessh-<id>-in.zip`, a separate name so that copying within one server works too.
- Unpack on server verifies the SHA-256 passed as an argument (exit 20), applies the existing-folder choice (backup, merge or stop: exit 21), then runs `unzip -o`. The fallback is a Python unpacker that restores modes and links and refuses `..` and absolute entries (exit 22).
- Set owner runs `chown -R` with a validated `user[:group]`; it needs root until sudo comes in phase 3.
- Server to server goes via this PC, using a temporary file or a folder for a kept copy.
- Clean up removes the archives on every side. A failed removal is a warning.
- Each server involved gets its own activity log entry.

**As built (M11 phase 3)**:

- Every remote script gets the login user's `$HOME` as an argument (expanded by the login shell before `sudo`), so `~` resolves to the login user's home even when the script runs as root.
- `sudo` is off by default and set separately for the source (check and zip) and the destination (check, unpack and set owner). The ssh session gets `-t`, so sudo can ask for a password in the tab. An archive zipped as root is handed back to the login user, who downloads and deletes it.
- Excludes are typed one per line. A plain name such as `node_modules` or `*.log` matches at any depth. A path with a slash, such as `storage/cache`, is relative to the folder. The patterns are turned into archive-path globs that `zip -x`, the Python packer and the local Rust packer all read the same way. An excluded folder is not entered.
- Scripts before (on the source, before zipping) and after (on the destination, after unpacking) can be a library script or a command line, run with a terminal so they can prompt. A failing script stops the transfer, and Retry continues from it.
- Recipes are stored in the `folder_recipes` table (migration 004): a name, unique ignoring case, and the spec as JSON. The last run's time and result are recorded. "Folder recipes" in the sidebar runs one as saved, opens one in the transfer dialog to change it for this run (or to update it), or deletes one. A recipe whose server was deleted cannot run.
- The remote folder browser uses `ssh -o BatchMode=yes` without a console window and lists one folder (`pwd`, then `ls -1Ap`, up to 2000 entries). It only works with key-based logins; for other hosts the path is typed.

**As built (M11 phase 4)**:

- "Stream directly between the servers" (`direct` in the spec, server → server only, not together with a kept local copy) replaces Download + Upload with one **Stream to destination** step (`folder/relay.rs`).
- The archive is still zipped into `/tmp` on the source first, by the usual Pack step (in the tab, so sudo and its prompt work). Then the step reads its `.sum`, tests the destination login, and runs `ssh A 'cat archive'` and `ssh B 'cat > inbox'` with piped stdio, copying between the two in the app. It counts the bytes, hashes them and shows progress (MB, %, MB/s). Nothing is written to this PC's disk.
- Both ssh processes run with `BatchMode=yes` and no console window, because their stdio carries the archive and there is no terminal to prompt in. A password-only login fails fast, with an explanation that points to the via-this-PC mode.
- The size and SHA-256 of the stream are compared with the source's `.sum`. The destination then checks the archive again before unpacking. If the source had no SHA-256 tool, the hash taken in transit is passed on, so the second leg is still verified.
- Because the source's archive stays in `/tmp` until Clean up, a failed or cancelled stream can be retried from that step without zipping again. That is better than the "no resume" in the original plan. Cancelling (closing the tab) kills both processes.

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
| M11 | Folder transfers     | zip recipes: check, pack, download/upload, unpack, scripts, saved recipes (see §8a)                              |

**As built (M9)**: a per-user NSIS installer (no admin rights; downloads WebView2 if it is missing). The updater plugin reads `latest.json` from the latest published GitHub release and checks its minisign signature. The public key is in `tauri.conf.json`; the private key and its password are kept outside the repo and stored as CI secrets. The app checks once on startup (there is a setting to turn this off) and again whenever the user clicks "Check now" in Settings → About. The update dialog shows the release notes and warns when terminal sessions are open. The NSIS installer then runs in passive mode and the app restarts. Before a newer schema is applied, the database is copied to `db.v<N>.bak`, and a database from a newer version is refused instead of being opened. Releases: `pnpm version:bump X.Y.Z` → `CHANGELOG.md` section → tag `vX.Y.Z` → the `release.yml` workflow builds the installer and creates a draft release. The draft becomes visible to the updater once it is published. Authenticode signing uses Azure Trusted Signing; it is switched on by repository variables and secrets, and builds are unsigned until those are set.

**Release pipeline (as built)**: `staging` is the working branch. CI checks every push to it and keeps a test installer as a workflow artifact; those builds have no updater files. Every push to `main` is a release, handled by `release.yml` on Windows:

- `scripts/version.mjs next` picks the version. It is the files' version when that is newer than the last tag; otherwise the patch number goes up, or the minor/major one when a commit message says `#minor`/`#major`.
- `prepare` sets the version in every file and renames `## Unreleased` in `CHANGELOG.md`; without that section, the commit subjects are listed.
- After the checks and the signed build, the workflow commits `Release vX.Y.Z` to `main` and tags it in one atomic push. It then publishes the installer, its `.sig`, `latest.json` and a copy under the stable name `RidgeSSH-setup.exe` in the public `Everridge/ridgessh-releases` repo (written by `scripts/publish.mjs`) and refreshes that repo's README. Last, it merges the tag back into `staging`.
- The source repo stays private, so the updater endpoint points at the public repo's `latest.json`.
- On startup the app opens the update dialog itself, unless the user chose "Skip this version" for that version. That choice is remembered in `localStorage`; "Check now" still shows the update.
- macOS releases are paused until signing is decided.

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
