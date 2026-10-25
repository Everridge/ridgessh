# Changelog

Each `## <version>` section becomes the GitHub release notes and the text shown in the in-app
update dialog, so write it for users.

## 0.1.0

First release, for Windows 10/11 and macOS 11+.

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
