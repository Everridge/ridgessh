import { mockIPC } from "@tauri-apps/api/mocks";
import { offeredOp } from "@/features/config-write/pending";
import type {
  AuditEntry,
  ConfigCandidate,
  ConfigChange,
  BackupInfo,
  BackupInspection,
  BackupStatus,
  ConfigPreview,
  Group,
  GroupInput,
  HistoryFilter,
  Host,
  HostInput,
  ImportPreview,
  ImportRequest,
  ImportResult,
  ImportSource,
  ExportSelection,
  RenameSuggestion,
  RestoreOptions,
  Script,
  ScriptInput,
  Settings,
  SettingsView,
  SyncReport,
  UserVariant,
} from "@/lib/ipc";

/**
 * In-memory stand-in for the Rust commands, good enough to drive the UI in tests.
 * Business rules are tested in Rust; this only mirrors the shapes and the obvious behaviour.
 */
export interface FakeSeed {
  hosts?: Partial<Host>[];
  groups?: Group[];
  /** What the startup `sync_run` reports (host states are taken from `hosts` as seeded). */
  report?: Partial<SyncReport>;
  renames?: RenameSuggestion[];
  settings?: Partial<Settings>;
  rememberedPassword?: string;
  /** Existing backup files and the password each was made with. */
  backups?: { fileName: string; password: string }[];
  /** Extra audit entries, oldest first. */
  history?: Omit<AuditEntry, "id">[];
  scripts?: Script[];
  lastRemote?: string;
  /** What the native file/folder picker returns. */
  dialogResult?: string | string[];
  /** What the native save dialog returns. */
  saveResult?: string;
  /** What `import_preview` shows, and the password an export source needs. */
  importPreview?: ImportPreview;
  importPassword?: string;
  /** Per-user Match blocks of the config. */
  variants?: UserVariant[];
}

export function installFakeBackend(seed: FakeSeed = {}) {
  let seq = 0;
  const id = () => `id-${++seq}`;
  const now = () => new Date(Date.UTC(2026, 9, 24, 10, 0, seq)).toISOString();
  const groups: Group[] = [...(seed.groups ?? [])];
  const hosts: Host[] = (seed.hosts ?? []).map((h) => makeHost({ id: id(), ...h }));
  const audit: AuditEntry[] = [];
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  let renames = [...(seed.renames ?? [])];
  let lastSync: SyncReport | null = null;
  let scripts: Script[] = [...(seed.scripts ?? [])];
  let settings: Settings = {
    backupDir: null,
    confirmationWord: "CONFIRM",
    autoBackup: true,
    autoBackupIntervalMin: 30,
    retentionCount: 30,
    retentionDays: 0,
    trashRetentionDays: 30,
    checkUpdates: true,
    terminalTheme: "auto",
    terminalFontFamily: null,
    terminalFontSize: 13,
    ...seed.settings,
  };
  let remembered: string | null = seed.rememberedPassword ?? null;
  let backups: BackupInfo[] = [];
  const backupPasswords = new Map<string, string>();
  for (const b of seed.backups ?? []) {
    const info = fakeBackup(b.fileName, true);
    backups.push(info);
    backupPasswords.set(info.path, b.password);
  }
  const settingsView = (): SettingsView => ({
    settings,
    backupDir: settings.backupDir ?? "C:\\data\\backups",
    defaultBackupDir: "C:\\data\\backups",
  });

  const log = (
    entityId: string,
    action: AuditEntry["action"],
    before: unknown,
    after: unknown,
    batchId: string | null = `batch-${audit.length + 1}`,
  ) =>
    audit.unshift({
      id: audit.length + 1,
      ts: now(),
      batchId,
      source: "user",
      entityType: "host",
      entityId,
      action,
      label: String(((after ?? before) as { alias?: string } | null)?.alias ?? entityId),
      before: before as AuditEntry["before"],
      after: after as AuditEntry["after"],
    });
  for (const e of seed.history ?? []) audit.unshift({ ...e, id: audit.length + 1 });

  const find = (hid: string) => {
    const h = hosts.find((x) => x.id === hid);
    if (!h) throw { kind: "not_found", message: `host ${hid}` };
    return h;
  };

  const applyInput = (h: Host, input: HostInput): Host => {
    const clash = hosts.find((x) => x.id !== h.id && !x.deletedAt && x.alias === input.alias);
    if (clash) throw { kind: "conflict", message: `a host named "${input.alias}" already exists` };
    return {
      ...h,
      ...input,
      tags: input.tags.map((name) => ({ id: `tag-${name}`, name, color: null })),
      updatedAt: now(),
    };
  };

  mockIPC((cmd, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args });
    switch (cmd) {
      case "app_info":
        return {
          version: "0.0.0",
          dataDir: "C:\\data",
          dbFile: "C:\\data\\ridgessh.db",
          defaultBackupDir: "C:\\data\\backups",
          sshConfig: "C:\\Users\\me\\.ssh\\config",
          sshConfigExists: true,
        };
      case "hosts_list":
        return structuredClone(hosts);
      case "host_create": {
        const h = applyInput(makeHost({ id: id(), createdAt: now() }), args.input as HostInput);
        hosts.push(h);
        log(h.id, "create", null, h);
        return h;
      }
      case "host_update": {
        const before = find(args.id as string);
        const after = applyInput(before, args.input as HostInput);
        hosts[hosts.indexOf(before)] = after;
        log(after.id, "update", before, after);
        return after;
      }
      case "host_set_favorite":
        find(args.id as string).favorite = args.favorite as boolean;
        return null;
      case "hosts_move":
        for (const hid of args.ids as string[])
          find(hid).groupId = (args.groupId as string) ?? null;
        return null;
      case "hosts_delete":
        for (const hid of args.ids as string[]) {
          const h = find(hid);
          log(hid, "delete", { ...h }, { ...h, deletedAt: now() });
          h.deletedAt = now();
        }
        return null;
      case "host_restore": {
        const h = find(args.id as string);
        h.deletedAt = null;
        log(h.id, "restore", null, h);
        return h;
      }
      case "host_purge":
        hosts.splice(hosts.indexOf(find(args.id as string)), 1);
        return null;
      case "host_history":
        return audit.filter((e) => e.entityId === args.id);
      case "history_list": {
        const f = (args.filter ?? {}) as HistoryFilter;
        return audit.filter(
          (e) =>
            (!f.entityType || e.entityType === f.entityType) &&
            (!f.source || e.source === f.source) &&
            (!f.search || JSON.stringify(e).includes(f.search)),
        );
      }
      case "history_revert":
      case "history_revert_batch":
        return args.batchId ? 1 : null;
      case "scripts_list":
        return [...scripts];
      case "script_create": {
        const s: Script = {
          id: id(),
          ...(args.input as ScriptInput),
          createdAt: now(),
          updatedAt: now(),
        };
        scripts.push(s);
        return s;
      }
      case "script_update": {
        const s = scripts.find((x) => x.id === args.id)!;
        Object.assign(s, args.input, { updatedAt: now() });
        return s;
      }
      case "script_delete":
        scripts = scripts.filter((x) => x.id !== args.id);
        return null;
      case "script_read_file":
        return "echo from file\n";
      case "transfer_last_remote":
        return seed.lastRemote ?? null;
      case "plugin:dialog|open":
        return seed.dialogResult ?? null;
      case "plugin:dialog|save":
        return seed.saveResult ?? null;
      case "export_create": {
        const sel = args.selection as ExportSelection;
        return {
          path: String(args.path),
          hosts: sel.hostIds.length,
          groups: 0,
          scripts: sel.scriptIds.length,
          keys: 0,
          warnings: [],
        };
      }
      case "export_config_file":
        return String(args.path);
      case "import_preview": {
        const source = args.source as ImportSource;
        if (source.kind === "export" && source.password !== seed.importPassword)
          throw { kind: "validation", message: "wrong password for this export" };
        if (!seed.importPreview) throw { kind: "validation", message: "not an export" };
        return { ...seed.importPreview, kind: source.kind };
      }
      case "import_apply": {
        const req = args.request as ImportRequest;
        const preview = seed.importPreview!;
        const r: ImportResult = {
          created: 0,
          overwritten: 0,
          copied: 0,
          skipped: 0,
          scripts: 0,
          groupsCreated: 0,
          keysInstalled: [],
          pendingHostIds: [],
          warnings: [],
          autoBackup: null,
        };
        for (const c of req.hosts) {
          const p = preview.hosts.find((h) => h.key === c.key)!;
          if (c.strategy === "skip") {
            r.skipped++;
            continue;
          }
          if (c.strategy === "overwrite") {
            r.overwritten++;
            continue;
          }
          const alias = c.strategy === "copy" ? p.copyAlias : p.alias;
          const h = makeHost({
            id: id(),
            alias,
            hostname: p.hostname,
            groupId: req.targetGroupId,
            syncState: "db_only",
          });
          hosts.push(h);
          r[c.strategy === "copy" ? "copied" : "created"]++;
          r.pendingHostIds.push(h.id);
        }
        r.scripts = req.scripts.filter((c) => c.strategy !== "skip").length;
        return r;
      }
      case "hosts_empty_trash": {
        const trashed = hosts.filter((h) => h.deletedAt);
        for (const h of trashed) hosts.splice(hosts.indexOf(h), 1);
        return { purged: trashed.length, keptInConfig: 0 };
      }
      case "groups_list":
        return structuredClone(groups);
      case "group_create": {
        const input = args.input as GroupInput;
        const g: Group = {
          id: id(),
          ...input,
          sortOrder: groups.length,
          createdAt: now(),
          updatedAt: now(),
        };
        groups.push(g);
        return g;
      }
      case "group_update": {
        const g = groups.find((x) => x.id === args.id)!;
        Object.assign(g, args.input);
        return g;
      }
      case "group_delete": {
        const g = groups.find((x) => x.id === args.id)!;
        groups.splice(groups.indexOf(g), 1);
        for (const h of hosts) if (h.groupId === g.id) h.groupId = g.parentId;
        return null;
      }
      case "tags_list":
        return [];
      case "terminal_open_external":
        return null;
      case "folder_last":
        return { remote: seed.lastRemote ?? null, local: null };
      case "config_variants":
        return seed.variants ?? [];
      case "host_activity":
        return [];
      case "sync_run":
        lastSync = {
          scannedAt: now(),
          configPath: "C:\\Users\\me\\.ssh\\config",
          configHosts: hosts.length,
          added: [],
          updated: [],
          removed: [],
          reappeared: [],
          conflicts: [],
          rules: [],
          warnings: [],
          ...seed.report,
        };
        return lastSync;
      case "sync_last":
        return lastSync;
      case "settings_get":
        return settingsView();
      case "settings_update":
        settings = args.settings as Settings;
        return settingsView();
      case "backup_status":
        return {
          passwordRemembered: remembered !== null,
          autoBackup: settings.autoBackup,
          backupDir: settings.backupDir ?? "C:\\data\\backups",
          lastBackup: backups[0] ?? null,
          lastError: null,
        } satisfies BackupStatus;
      case "backup_password_remember":
        remembered = args.password as string;
        return null;
      case "backup_password_forget":
        remembered = null;
        return null;
      case "backups_list":
        return [...backups];
      case "backup_create": {
        const password = (args.password as string | undefined) ?? remembered;
        if (!password) throw { kind: "validation", message: "enter a backup password" };
        const b = fakeBackup(`ridgessh-${++seq}-manual.rsbak`, true);
        backupPasswords.set(b.path, password);
        backups.unshift(b);
        return b;
      }
      case "backup_delete":
        backups = backups.filter((b) => b.fileName !== args.fileName);
        return null;
      case "backup_inspect": {
        const path = args.path as string;
        if (backupPasswords.get(path) !== args.password)
          throw { kind: "validation", message: "wrong password for this backup" };
        return {
          path,
          manifest: {
            format: 1,
            appVersion: "0.1.0",
            createdAt: "2026-10-20T08:00:00.000Z",
            reason: "manual",
            manual: true,
            hostCount: 1,
            groupCount: 0,
            configPath: "C:\\Users\\me\\.ssh\\config",
            files: [],
          },
          hosts: { total: 1, added: ["old-box"], removed: hosts.map((h) => h.alias), changed: [] },
          config: {
            diff: [{ kind: "add", text: "Host old-box", old: null, new: 1 }],
            additions: 1,
            deletions: 0,
            baseHash: "cfg-hash",
          },
          configPath: "C:\\Users\\me\\.ssh\\config",
          confirmationWord: settings.confirmationWord,
        } satisfies BackupInspection;
      }
      case "backup_restore": {
        const o = args.options as RestoreOptions;
        if (o.restoreConfig && o.confirmation !== settings.confirmationWord)
          throw { kind: "validation", message: "the confirmation word does not match" };
        const safety = fakeBackup(`ridgessh-${++seq}-auto-before-restore.rsbak`, false);
        backups.unshift(safety);
        return {
          restoredDb: o.restoreDb,
          restoredConfig: o.restoreConfig,
          safetyBackup: safety,
          previousConfig: null,
          report: lastSync,
        };
      }
      case "config_candidates":
        return hosts.flatMap((h): ConfigCandidate[] => {
          const op = offeredOp(h);
          if (!op) return [];
          const conflict = h.syncState === "conflict";
          return [
            {
              hostId: h.id,
              alias: h.alias,
              op,
              selected: op === "update" && !conflict,
              blocked: conflict ? "Resolve the conflict with the config first." : null,
            },
          ];
        });
      case "config_preview": {
        const changes = args.changes as ConfigChange[];
        return {
          configPath: "C:\\Users\\me\\.ssh\\config",
          fileExists: true,
          baseHash: "base",
          resultHash: `result:${changes.map((c) => c.hostId).join(",")}`,
          diff: changes.flatMap((c) => [
            { kind: "hunk", text: "@@ -1,1 +1,2 @@", old: null, new: null },
            { kind: "context", text: `Host ${find(c.hostId).alias}`, old: 1, new: 1 },
            { kind: "add", text: "  Port 2222", old: null, new: 2 },
          ]),
          additions: changes.length,
          deletions: 0,
          confirmationWord: "CONFIRM",
        } satisfies ConfigPreview;
      }
      case "config_apply": {
        if (args.confirmation !== "CONFIRM")
          throw { kind: "validation", message: "the confirmation word does not match" };
        const changes = args.changes as ConfigChange[];
        for (const c of changes) {
          const h = find(c.hostId);
          if (c.op === "remove") Object.assign(h, { syncState: "db_only", configSnapshot: null });
          else
            Object.assign(h, {
              syncState: "synced",
              configDirty: false,
              configSnapshot: {
                alias: h.alias,
                hostname: h.hostname,
                user: h.user,
                port: h.port,
                identityFile: h.identityFile,
                proxyJump: h.proxyJump,
                extraOptions: h.extraOptions,
              },
            });
        }
        return {
          configPath: "C:\\Users\\me\\.ssh\\config",
          previousCopy: "C:\\data\\config-history\\config-1",
          autoBackup: null,
          changed: changes.length,
          report: lastSync,
        };
      }
      case "sync_rename_suggestions":
        return renames;
      case "sync_acknowledge":
        for (const hid of args.ids as string[]) {
          const h = find(hid);
          if (h.syncState === "config_only_new") h.syncState = "synced";
        }
        renames = renames.filter((r) => !(args.ids as string[]).includes(r.toId));
        return null;
      case "sync_keep_db_only":
        for (const hid of args.ids as string[]) {
          Object.assign(find(hid), { syncState: "db_only", configSnapshot: null });
        }
        return null;
      case "sync_resolve_conflict": {
        const h = find(args.id as string);
        if (args.keep === "config") Object.assign(h, h.configSnapshot, { configDirty: false });
        else h.configDirty = true;
        h.syncState = "synced";
        return h;
      }
      case "sync_accept_rename": {
        const from = find(args.fromId as string);
        const to = find(args.toId as string);
        Object.assign(to, { notes: from.notes, groupId: from.groupId, syncState: "synced" });
        from.deletedAt = now();
        renames = renames.filter((r) => r.fromId !== from.id);
        return to;
      }
      default:
        throw new Error(`fake backend: unhandled command ${cmd}`);
    }
  });

  return { hosts, groups, calls };
}

export function makeHost(over: Partial<Host> = {}): Host {
  return {
    id: "h",
    alias: "host",
    hostname: null,
    user: null,
    port: null,
    identityFile: null,
    proxyJump: null,
    extraOptions: [],
    groupId: null,
    description: "",
    notes: "",
    color: null,
    favorite: false,
    syncState: "db_only",
    configDirty: false,
    configSnapshot: null,
    configSource: null,
    lastSeenInConfigAt: null,
    removedFromConfigAt: null,
    lastConnectedAt: null,
    deletedAt: null,
    createdAt: "2026-10-24T10:00:00.000Z",
    updatedAt: "2026-10-24T10:00:00.000Z",
    tags: [],
    fields: [],
    ...over,
  };
}

function fakeBackup(fileName: string, manual: boolean): BackupInfo {
  return {
    fileName,
    path: `C:\\data\\backups\\${fileName}`,
    createdAt: "2026-10-20T08:00:00.000Z",
    reason: manual ? "manual" : "before-restore",
    manual,
    size: 4096,
  };
}
