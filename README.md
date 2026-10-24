# RidgeSSH

Lightweight desktop SSH connection manager (Tauri 2 + React + shadcn/ui). See [docs/PLAN.md](docs/PLAN.md).

## Prerequisites

- Node 22+, pnpm 10+
- Rust (stable) with the MSVC toolchain on Windows, plus WebView2 (preinstalled on Windows 11)
- OpenSSH client (`ssh`, `scp`) on `PATH`

## Commands

| Command                         | What it does                                        |
| ------------------------------- | --------------------------------------------------- |
| `pnpm install`                  | install frontend deps                               |
| `pnpm tauri dev`                | run the app with hot reload                         |
| `pnpm tauri build`              | build the NSIS installer (needs the updater key)    |
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

App data lives in `%APPDATA%\com.everridge.ridgessh` (database, default backup folder).
