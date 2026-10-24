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
| `pnpm tauri build`              | build NSIS + MSI installers                         |
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

App data lives in `%APPDATA%\com.everridge.ridgessh` (database, default backup folder).
