# Changelog

Each `## <version>` section becomes the GitHub release notes and the text shown in the in-app
update dialog, so write it for users. Add changes under `## Unreleased`: the release workflow
renames it to the version it releases. Without it, the commit subjects are listed instead.

## Unreleased

- Drag hosts onto a group in the sidebar to move them there (drag one, or all checked hosts).
- New hosts can get their own key: tick "Generate a new key pair" and RidgeSSH creates
  `~/.ssh/<alias>_ed25519` together with the host (existing files are never overwritten).
- "Install the public key on the server" opens a terminal tab that logs in with the server's
  password once (typed at ssh's own prompt, never stored) and adds the key to
  `~/.ssh/authorized_keys`, like `ssh-copy-id`.
- For a reinstalled server or a reused address, "Forget the server's saved host key first" removes
  its old known_hosts entry (a `.old` copy is kept); ssh then asks you to confirm the new
  fingerprint.

## 0.1.0

First release, for Windows 10 and 11.

- Hosts and groups on top of your `~/.ssh/config`: descriptions, notes, tags and favorites live in
  RidgeSSH's own database, and the config file is only written after you type the confirmation
  word.
- The config is scanned on every start; changes made outside RidgeSSH are listed for review.
- Built-in terminal tabs with colour schemes, quick connect (Ctrl+K, ⌘K on macOS), scp file copy
  and remote scripts.
- Folder transfers as zip: server → this PC, this PC → server, and server → server (via this PC,
  or streamed directly), with checksums, excludes, sudo, scripts before and after, and saved
  recipes.
- Encrypted backups (automatic before risky changes) with restore preview.
- History of every change with undo, and a trash for deleted hosts.
- Encrypted export and import with per-host merge choices; plain ssh config import and export.
- Automatic updates.
