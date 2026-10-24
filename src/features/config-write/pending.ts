import type { ConfigOp, Host } from "@/lib/ipc";

/**
 * What writing this host to ~/.ssh/config would do. Mirrors `offered_op` in
 * `src-tauri/src/config_write/mod.rs`; the backend decides, this is only for labels and counts.
 */
export function offeredOp(h: Host): ConfigOp | null {
  if (h.deletedAt) {
    return h.configSnapshot && h.syncState !== "removed_from_config" ? "remove" : null;
  }
  if (h.syncState === "removed_from_config" || h.syncState === "db_only") return "add";
  if (h.syncState === "conflict") return "update";
  return h.configDirty && h.configSnapshot ? "update" : null;
}

/** Edits waiting to be written (what the top bar counts). */
export const isPendingWrite = (h: Host) =>
  !h.deletedAt && h.configDirty && h.configSnapshot !== null && h.syncState !== "conflict";

export function opLabel(op: ConfigOp, host?: Host): string {
  if (op === "update") return "Modify";
  if (op === "remove") return "Remove";
  return host?.syncState === "removed_from_config" ? "Restore" : "Add";
}

/** Button text for the action on one host. */
export function opAction(op: ConfigOp, host?: Host): string {
  if (op === "update") return "Write to config…";
  if (op === "remove") return "Remove from config…";
  return host?.syncState === "removed_from_config" ? "Restore into config…" : "Add to config…";
}
