# RidgeSSH

Lightweight desktop SSH connection manager (Tauri 2 + React + shadcn/ui). See [docs/PLAN.md](docs/PLAN.md).

## Download

[Latest Windows installer](https://github.com/Everridge/ridgessh/releases/latest/download/RidgeSSH-setup.exe)
· [all releases](https://github.com/Everridge/ridgessh/releases). The installed app
checks for updates when it starts and offers to install them.

## Branches and releases

- Work is pushed to **`staging`**. Each push runs the checks; nothing is built or released.
- **`main`** (the default branch) changes only through pull requests from `staging`.
- Merging such a pull request releases it (`.github/workflows/release.yml`): the patch
  version goes up (or minor/major when a commit message says `#minor` / `#major`, or when the
  version was raised with `pnpm version:bump X.Y.Z`), the `## Unreleased` section of
  `CHANGELOG.md` becomes the release notes, and the signed installer is published as a
  [GitHub release](https://github.com/Everridge/ridgessh/releases) of this repo, where the
  updater looks. The workflow commits `Release vX.Y.Z` to `main`, tags it and merges it back into
  `staging`.

## Prerequisites

- Node 22+, pnpm 10+
- Rust (stable): with the MSVC toolchain and WebView2 on Windows (WebView2 is preinstalled on Windows 11), or with the Xcode command line tools on macOS
- OpenSSH client (`ssh`, `scp`) on `PATH`

## Commands

| Command                         | What it does                                        |
| ------------------------------- | --------------------------------------------------- |
| `pnpm install`                  | install frontend deps                               |
| `pnpm tauri dev`                | run the app with hot reload                         |
| `pnpm tauri build`              | build the installer (Windows NSIS / macOS app+dmg)  |
| `pnpm check`                    | typecheck, lint, frontend tests, clippy, Rust tests |
| `pnpm format` / `pnpm rust:fmt` | format frontend / Rust code                         |

Add shadcn components with `pnpm dlx shadcn@latest add <name>`.

> Keep `@tauri-apps/*` npm packages on the same minor version as the Rust crates in
> `src-tauri/Cargo.lock` — `tauri dev` refuses to start on a mismatch.

## Layout

```
src/                    React app
  components/ui/        generated shadcn components
  components/layout/    app shell (sidebar, top bar)
  features/             hosts, terminal, … (one folder per feature)
  lib/ipc.ts            typed wrappers for Rust commands
  stores/               Zustand UI state
src-tauri/src/
  commands/             #[tauri::command] handlers
  error.rs, paths.rs    shared error type, filesystem locations
  security/             argument validation
  db, ssh_config, sync, terminal, backup, …   domain modules (filled in per milestone)
```

## Releasing

1. `pnpm version:bump 0.2.0` updates `package.json`, `Cargo.toml`, `Cargo.lock` and `tauri.conf.json`.
2. Add a `## 0.2.0` section to `CHANGELOG.md`. It is used for the release notes and the in-app update dialog.
3. Commit, then `git tag v0.2.0 && git push origin main v0.2.0`.
4. The **Release** workflow builds the installer and creates a **draft** GitHub release. Publish it,
   and installed apps will offer the update on their next start.

The updater checks signatures with the key pair made by `pnpm tauri signer generate`. The public key is in
`tauri.conf.json`. The private key and its password must be set as the repository secrets
`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. A local `pnpm tauri build` needs
the same two environment variables. If the private key is lost, installed apps can no longer be updated
automatically.

Code signing (Authenticode) is optional and uses Azure Trusted Signing. To turn it on, set the repository
variables `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT` and `AZURE_SIGNING_PROFILE`, plus the secrets
`AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` and `AZURE_TENANT_ID`. Without them the installer is unsigned,
and SmartScreen shows "Windows protected your PC" (More info → Run anyway).

The release workflow also builds a universal macOS app. To sign and notarize it you need an Apple Developer
account and these secrets: `APPLE_CERTIFICATE` (a base64 `.p12` of a Developer ID Application certificate),
`APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (an app-specific
password) and `APPLE_TEAM_ID`. Without them the app is only ad-hoc signed, and Gatekeeper blocks it on first
launch. To open it anyway: System Settings → Privacy & Security → Open Anyway, or
`xattr -dr com.apple.quarantine /Applications/RidgeSSH.app`.

The app data folder, which holds the database and the default backup folder, is `%APPDATA%\com.everridge.ridgessh` on Windows and
`~/Library/Application Support/com.everridge.ridgessh` on macOS.
