import { invoke } from "@tauri-apps/api/core";

/** Error shape returned by every Rust command (see `src-tauri/src/error.rs`). */
export interface AppError {
  kind: "io" | "db" | "json" | "validation" | "conflict" | "not_found" | "other";
  message: string;
}

export function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "kind" in e && "message" in e;
}

export function errorMessage(e: unknown): string {
  if (isAppError(e)) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}

export interface AppInfo {
  version: string;
  dataDir: string;
  dbFile: string;
  defaultBackupDir: string;
  sshConfig: string;
  sshConfigExists: boolean;
}

export type SyncState =
  "synced" | "config_only_new" | "removed_from_config" | "db_only" | "conflict";

export interface Directive {
  key: string;
  value: string;
}

export interface HostField {
  key: string;
  value: string;
}

export interface Tag {
  id: string;
  name: string;
  color: string | null;
}

export interface TagWithCount extends Tag {
  hostCount: number;
}

export interface Host {
  id: string;
  alias: string;
  hostname: string | null;
  user: string | null;
  port: number | null;
  identityFile: string | null;
  proxyJump: string | null;
  extraOptions: Directive[];
  groupId: string | null;
  description: string;
  notes: string;
  color: string | null;
  favorite: boolean;
  syncState: SyncState;
  configDirty: boolean;
  lastSeenInConfigAt: string | null;
  removedFromConfigAt: string | null;
  lastConnectedAt: string | null;
  deletedAt: string | null;
  createdAt: string;
  updatedAt: string;
  tags: Tag[];
  fields: HostField[];
}

export interface HostInput {
  alias: string;
  hostname: string | null;
  user: string | null;
  port: number | null;
  identityFile: string | null;
  proxyJump: string | null;
  extraOptions: Directive[];
  groupId: string | null;
  description: string;
  notes: string;
  color: string | null;
  favorite: boolean;
  tags: string[];
  fields: HostField[];
}

export interface Group {
  id: string;
  parentId: string | null;
  name: string;
  color: string | null;
  icon: string | null;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

export interface GroupInput {
  name: string;
  parentId: string | null;
  color: string | null;
  icon: string | null;
}

export interface AuditEntry {
  id: number;
  ts: string;
  batchId: string | null;
  source: "user" | "sync" | "import" | "restore";
  entityType: string;
  entityId: string;
  action: "create" | "update" | "delete" | "restore" | "purge";
  before: Record<string, unknown> | null;
  after: Record<string, unknown> | null;
}

/** Typed wrappers around Tauri commands. Keep in sync with `src-tauri/src/commands`. */
export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),

  hostsList: () => invoke<Host[]>("hosts_list"),
  hostGet: (id: string) => invoke<Host>("host_get", { id }),
  hostCreate: (input: HostInput) => invoke<Host>("host_create", { input }),
  hostUpdate: (id: string, input: HostInput) => invoke<Host>("host_update", { id, input }),
  hostSetFavorite: (id: string, favorite: boolean) =>
    invoke<void>("host_set_favorite", { id, favorite }),
  hostsMove: (ids: string[], groupId: string | null) =>
    invoke<void>("hosts_move", { ids, groupId }),
  hostsDelete: (ids: string[]) => invoke<void>("hosts_delete", { ids }),
  hostRestore: (id: string) => invoke<Host>("host_restore", { id }),
  hostPurge: (id: string) => invoke<void>("host_purge", { id }),
  hostHistory: (id: string, limit?: number) => invoke<AuditEntry[]>("host_history", { id, limit }),

  groupsList: () => invoke<Group[]>("groups_list"),
  groupCreate: (input: GroupInput) => invoke<Group>("group_create", { input }),
  groupUpdate: (id: string, input: GroupInput) => invoke<Group>("group_update", { id, input }),
  groupDelete: (id: string) => invoke<void>("group_delete", { id }),

  tagsList: () => invoke<TagWithCount[]>("tags_list"),
  tagDelete: (id: string) => invoke<void>("tag_delete", { id }),
};
