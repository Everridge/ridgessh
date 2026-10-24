import { invoke } from "@tauri-apps/api/core";

/** Error shape returned by every Rust command (see `src-tauri/src/error.rs`). */
export interface AppError {
  kind: "io" | "validation" | "not_found" | "other";
  message: string;
}

export function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "kind" in e && "message" in e;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  dbFile: string;
  defaultBackupDir: string;
  sshConfig: string;
  sshConfigExists: boolean;
}

/** Typed wrappers around Tauri commands. Keep in sync with `src-tauri/src/commands`. */
export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
};
