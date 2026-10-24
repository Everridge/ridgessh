import { mockIPC } from "@tauri-apps/api/mocks";
import type {
  AuditEntry,
  Group,
  GroupInput,
  Host,
  HostInput,
  RenameSuggestion,
  SyncReport,
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

  const log = (entityId: string, action: AuditEntry["action"], before: unknown, after: unknown) =>
    audit.unshift({
      id: audit.length + 1,
      ts: now(),
      batchId: null,
      source: "user",
      entityType: "host",
      entityId,
      action,
      before: before as AuditEntry["before"],
      after: after as AuditEntry["after"],
    });

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
