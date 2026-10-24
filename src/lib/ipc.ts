import { Channel, invoke } from "@tauri-apps/api/core";

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

/** Connection part of a host, as it maps to a `Host` block in ssh_config. */
export interface Connection {
  alias: string;
  hostname: string | null;
  user: string | null;
  port: number | null;
  identityFile: string | null;
  proxyJump: string | null;
  extraOptions: Directive[];
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
  /** What ~/.ssh/config said at the last scan; null if the host is not in the config. */
  configSnapshot: Connection | null;
  /** Included file the host comes from; null for the main config file. */
  configSource: string | null;
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

export interface RuleBlock {
  kind: "host" | "match";
  header: string;
  source: string;
  line: number;
  directiveCount: number;
}

export interface SyncReport {
  scannedAt: string;
  configPath: string;
  configHosts: number;
  added: string[];
  updated: string[];
  removed: string[];
  reappeared: string[];
  conflicts: string[];
  rules: RuleBlock[];
  warnings: string[];
}

export interface RenameSuggestion {
  fromId: string;
  toId: string;
}

export type HistorySource = "user" | "sync" | "import" | "restore" | "system";

export interface HistoryFilter {
  entityType?: AuditEntry["entityType"] | null;
  source?: HistorySource | null;
  search?: string | null;
  /** Page cursor: entries older than this id. */
  beforeId?: number | null;
  limit?: number;
}

export interface AuditEntry {
  id: number;
  ts: string;
  batchId: string | null;
  source: HistorySource;
  entityType: "host" | "group" | "tag" | "settings" | "backup";
  entityId: string;
  action: "create" | "update" | "delete" | "restore" | "purge";
  /** Host alias, group name, "Settings", backup file. */
  label: string;
  before: Record<string, unknown> | null;
  after: Record<string, unknown> | null;
}

export interface Activity {
  id: number;
  hostId: string | null;
  kind: "terminal" | "terminal_external" | "scp_up" | "scp_down" | "script";
  startedAt: string;
  endedAt: string | null;
  exitCode: number | null;
  details: { command?: string } | null;
}

/** What writing a host to ~/.ssh/config would do (see `src-tauri/src/config_write`). */
export type ConfigOp = "update" | "add" | "remove";

export interface ConfigChange {
  hostId: string;
  op: ConfigOp;
}

export interface ConfigCandidate extends ConfigChange {
  alias: string;
  /** Pre-selected: pending edits are, additions and removals are opt-in. */
  selected: boolean;
  /** Why it cannot be written right now. */
  blocked: string | null;
}

export interface DiffLine {
  kind: "hunk" | "context" | "add" | "remove";
  text: string;
  old: number | null;
  new: number | null;
}

export interface ConfigPreview {
  configPath: string;
  fileExists: boolean;
  baseHash: string;
  resultHash: string;
  diff: DiffLine[];
  additions: number;
  deletions: number;
  confirmationWord: string;
}

export interface ConfigApplyResult {
  configPath: string;
  /** Plain copy of the previous file (in the app data folder). */
  previousCopy: string | null;
  /** Encrypted automatic backup made just before writing. */
  autoBackup: BackupInfo | null;
  changed: number;
  report: SyncReport;
}

export interface Settings {
  /** null = default folder under the app data dir. */
  backupDir: string | null;
  confirmationWord: string;
  autoBackup: boolean;
  /** Ordinary edits back up at most once per this many minutes (0 = never). */
  autoBackupIntervalMin: number;
  retentionCount: number;
  /** 0 = no age limit. */
  retentionDays: number;
  /** Trashed hosts are deleted permanently after this many days (0 = never). */
  trashRetentionDays: number;
}

export interface SettingsView {
  settings: Settings;
  backupDir: string;
  defaultBackupDir: string;
}

export interface BackupInfo {
  fileName: string;
  path: string;
  createdAt: string;
  /** manual, config-write, before-restore, bulk-delete, sync, periodic, import or unknown. */
  reason: string;
  manual: boolean;
  size: number;
}

export interface BackupStatus {
  passwordRemembered: boolean;
  autoBackup: boolean;
  backupDir: string;
  lastBackup: BackupInfo | null;
  lastError: string | null;
}

export interface BackupManifest {
  format: number;
  appVersion: string;
  createdAt: string;
  reason: string;
  manual: boolean;
  hostCount: number;
  groupCount: number;
  configPath: string;
  files: { name: string; sha256: string; size: number; originalPath: string | null }[];
}

export interface BackupInspection {
  path: string;
  manifest: BackupManifest;
  hosts: { total: number; added: string[]; removed: string[]; changed: string[] };
  config: { diff: DiffLine[]; additions: number; deletions: number; baseHash: string } | null;
  configPath: string;
  confirmationWord: string;
}

export interface RestoreOptions {
  restoreDb: boolean;
  restoreConfig: boolean;
  configBaseHash: string | null;
  confirmation: string | null;
}

export interface RestoreResult {
  restoredDb: boolean;
  restoredConfig: boolean;
  safetyBackup: BackupInfo;
  previousConfig: string | null;
  report: SyncReport;
}

export interface TransferSpec {
  hostId: string;
  direction: "upload" | "download";
  /** Upload: files/folders to send. Download: one local folder to save into. */
  local: string[];
  remote: string;
  recursive: boolean;
}

export type RunMode = "stream" | "copy";

export interface Script {
  id: string;
  name: string;
  description: string;
  content: string;
  interpreter: string;
  defaultArgs: string;
  runMode: RunMode;
  createdAt: string;
  updatedAt: string;
}

export type ScriptInput = Pick<
  Script,
  "name" | "description" | "content" | "interpreter" | "defaultArgs" | "runMode"
>;

/** A library script (`scriptId`) or ad-hoc content, for one host. */
export interface ScriptRun {
  hostId: string;
  scriptId?: string | null;
  name?: string | null;
  content?: string | null;
  interpreter?: string | null;
  mode?: RunMode | null;
  args: string;
}

export interface TerminalOpened {
  sessionId: string;
  command: string;
}

export interface TerminalExit {
  exitCode: number | null;
}

export interface ExportSelection {
  hostIds: string[];
  scriptIds: string[];
  /** Include the private key files hosts name in IdentityFile. */
  includeKeys: boolean;
}

export interface ExportSummary {
  path: string;
  hosts: number;
  groups: number;
  scripts: number;
  keys: number;
  warnings: string[];
}

export type ImportSource =
  { kind: "export"; path: string; password: string } | { kind: "config"; path: string };

export type ItemStatus = "new" | "same" | "differs";
export type ImportStrategy = "skip" | "import" | "overwrite" | "copy";

export interface HostImportPreview {
  key: string;
  alias: string;
  hostname: string | null;
  user: string | null;
  port: number | null;
  groupPath: string[];
  status: ItemStatus;
  existingId: string | null;
  differences: string[];
  copyAlias: string;
  suggested: ImportStrategy;
  note: string | null;
}

export interface ScriptImportPreview {
  key: string;
  name: string;
  status: ItemStatus;
  existingId: string | null;
  differences: string[];
  copyName: string;
  suggested: ImportStrategy;
}

export interface KeyImportPreview {
  identityFile: string;
  target: string | null;
  status: "new" | "same" | "exists" | "unsupported";
  hasPublicKey: boolean;
}

export interface ImportPreview {
  kind: "export" | "config";
  fileName: string;
  createdAt: string | null;
  appVersion: string | null;
  hosts: HostImportPreview[];
  scripts: ScriptImportPreview[];
  keys: KeyImportPreview[];
  warnings: string[];
}

export interface ImportChoice {
  key: string;
  strategy: ImportStrategy;
}

export interface ImportRequest {
  targetGroupId: string | null;
  hosts: ImportChoice[];
  scripts: ImportChoice[];
  installKeys: boolean;
}

export interface ImportResult {
  created: number;
  overwritten: number;
  copied: number;
  skipped: number;
  scripts: number;
  groupsCreated: number;
  keysInstalled: string[];
  /** Imported hosts with connection values to write to ~/.ssh/config. */
  pendingHostIds: string[];
  warnings: string[];
  autoBackup: BackupInfo | null;
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
  hostsEmptyTrash: () => invoke<{ purged: number; keptInConfig: number }>("hosts_empty_trash"),

  historyList: (filter: HistoryFilter) => invoke<AuditEntry[]>("history_list", { filter }),
  /** `before` undoes the change, `after` goes back to the version it produced. */
  historyRevert: (id: number, target: "before" | "after") =>
    invoke<void>("history_revert", { id, target }),
  historyRevertBatch: (batchId: string) => invoke<number>("history_revert_batch", { batchId }),

  groupsList: () => invoke<Group[]>("groups_list"),
  groupCreate: (input: GroupInput) => invoke<Group>("group_create", { input }),
  groupUpdate: (id: string, input: GroupInput) => invoke<Group>("group_update", { id, input }),
  groupDelete: (id: string) => invoke<void>("group_delete", { id }),

  tagsList: () => invoke<TagWithCount[]>("tags_list"),
  tagDelete: (id: string) => invoke<void>("tag_delete", { id }),

  syncRun: () => invoke<SyncReport>("sync_run"),
  syncLast: () => invoke<SyncReport | null>("sync_last"),
  syncRenameSuggestions: () => invoke<RenameSuggestion[]>("sync_rename_suggestions"),
  syncAcknowledge: (ids: string[]) => invoke<void>("sync_acknowledge", { ids }),
  syncKeepDbOnly: (ids: string[]) => invoke<void>("sync_keep_db_only", { ids }),
  syncResolveConflict: (id: string, keep: "config" | "database") =>
    invoke<Host>("sync_resolve_conflict", { id, keep }),
  syncAcceptRename: (fromId: string, toId: string) =>
    invoke<Host>("sync_accept_rename", { fromId, toId }),

  configCandidates: () => invoke<ConfigCandidate[]>("config_candidates"),
  configPreview: (changes: ConfigChange[]) => invoke<ConfigPreview>("config_preview", { changes }),
  configApply: (
    changes: ConfigChange[],
    baseHash: string,
    resultHash: string,
    confirmation: string,
  ) => invoke<ConfigApplyResult>("config_apply", { changes, baseHash, resultHash, confirmation }),

  settingsGet: () => invoke<SettingsView>("settings_get"),
  settingsUpdate: (settings: Settings) => invoke<SettingsView>("settings_update", { settings }),

  backupStatus: () => invoke<BackupStatus>("backup_status"),
  backupPasswordRemember: (password: string) =>
    invoke<void>("backup_password_remember", { password }),
  backupPasswordForget: () => invoke<void>("backup_password_forget"),
  backupsList: () => invoke<BackupInfo[]>("backups_list"),
  /** Without a password the remembered one is used. */
  backupCreate: (password?: string) => invoke<BackupInfo>("backup_create", { password }),
  backupDelete: (fileName: string) => invoke<void>("backup_delete", { fileName }),
  backupInspect: (path: string, password: string) =>
    invoke<BackupInspection>("backup_inspect", { path, password }),
  backupRestore: (path: string, password: string, options: RestoreOptions) =>
    invoke<RestoreResult>("backup_restore", { path, password, options }),

  terminalOpen: (
    hostId: string,
    cols: number,
    rows: number,
    onData: Channel<ArrayBuffer>,
    onExit: Channel<TerminalExit>,
  ) => invoke<TerminalOpened>("terminal_open", { hostId, cols, rows, onData, onExit }),
  terminalWrite: (sessionId: string, data: string) =>
    invoke<void>("terminal_write", { sessionId, data }),
  terminalResize: (sessionId: string, cols: number, rows: number) =>
    invoke<void>("terminal_resize", { sessionId, cols, rows }),
  terminalClose: (sessionId: string) => invoke<void>("terminal_close", { sessionId }),
  terminalOpenExternal: (hostId: string) => invoke<void>("terminal_open_external", { hostId }),
  transferOpen: (
    spec: TransferSpec,
    cols: number,
    rows: number,
    onData: Channel<ArrayBuffer>,
    onExit: Channel<TerminalExit>,
  ) => invoke<TerminalOpened>("transfer_open", { spec, cols, rows, onData, onExit }),
  transferLastRemote: (hostId: string) => invoke<string | null>("transfer_last_remote", { hostId }),
  scriptsList: () => invoke<Script[]>("scripts_list"),
  scriptCreate: (input: ScriptInput) => invoke<Script>("script_create", { input }),
  scriptUpdate: (id: string, input: ScriptInput) => invoke<Script>("script_update", { id, input }),
  scriptDelete: (id: string) => invoke<void>("script_delete", { id }),
  scriptReadFile: (path: string) => invoke<string>("script_read_file", { path }),
  scriptOpen: (
    run: ScriptRun,
    cols: number,
    rows: number,
    onData: Channel<ArrayBuffer>,
    onExit: Channel<TerminalExit>,
  ) => invoke<TerminalOpened>("script_open", { run, cols, rows, onData, onExit }),
  exportCreate: (path: string, password: string, selection: ExportSelection) =>
    invoke<ExportSummary>("export_create", { path, password, selection }),
  /** Plain, unencrypted ssh_config text with the hosts' connection values. */
  exportConfigFile: (path: string, hostIds: string[]) =>
    invoke<string>("export_config_file", { path, hostIds }),
  importPreview: (source: ImportSource) => invoke<ImportPreview>("import_preview", { source }),
  importApply: (source: ImportSource, request: ImportRequest) =>
    invoke<ImportResult>("import_apply", { source, request }),
  hostActivity: (hostId: string, limit?: number) =>
    invoke<Activity[]>("host_activity", { hostId, limit }),
};
