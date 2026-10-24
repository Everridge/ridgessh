# Changelog

Each `## <version>` section becomes the GitHub release notes and the text shown in the in-app
update dialog, so write it for users.

## 0.1.0

First release.

- Hosts and groups on top of your `~/.ssh/config`: descriptions, notes, tags and favorites live in
  RidgeSSH's own database, and the config file is only written after you type the confirmation
  word.
- The config is scanned on every start; changes made outside RidgeSSH are listed for review.
- Built-in terminal tabs, quick connect (Ctrl+K), scp file copy and remote scripts.
- Encrypted backups (automatic before risky changes) with restore preview.
- History of every change with undo, and a trash for deleted hosts.
- Encrypted export and import with per-host merge choices; plain ssh config import and export.
- Automatic updates.
