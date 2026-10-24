export interface FieldChange {
  field: string;
  before: string;
  after: string;
}

/** Bookkeeping columns that change on every write and say nothing to the user. */
const IGNORED = new Set(["id", "updatedAt", "createdAt"]);

const LABELS: Record<string, string> = {
  alias: "Alias",
  hostname: "Hostname",
  user: "User",
  port: "Port",
  identityFile: "Identity file",
  proxyJump: "ProxyJump",
  extraOptions: "Options",
  groupId: "Group",
  description: "Description",
  notes: "Notes",
  color: "Color",
  favorite: "Favorite",
  syncState: "Sync state",
  configDirty: "Pending config change",
  deletedAt: "Deleted",
  tags: "Tags",
  fields: "Custom fields",
  name: "Name",
  parentId: "Parent",
  backupDir: "Backup folder",
  confirmationWord: "Confirmation word",
  autoBackup: "Automatic backups",
  autoBackupIntervalMin: "Backup interval (min)",
  retentionCount: "Backups kept",
  retentionDays: "Backup age limit (days)",
  trashRetentionDays: "Trash kept (days)",
  checkUpdates: "Check for updates",
};

export function fieldLabel(field: string): string {
  return LABELS[field] ?? field;
}

function show(v: unknown): string {
  if (v === null || v === undefined || v === "") return "—";
  if (typeof v === "boolean") return v ? "yes" : "no";
  if (Array.isArray(v)) {
    if (v.length === 0) return "—";
    return v
      .map((x) => {
        if (x && typeof x === "object") {
          const o = x as Record<string, unknown>;
          if ("name" in o) return String(o.name);
          if ("key" in o) return `${String(o.key)}=${String(o.value)}`;
        }
        return String(x);
      })
      .join(", ");
  }
  if (typeof v === "object") return JSON.stringify(v);
  return String(v);
}

/** Field-level differences between two entity snapshots from the audit log. */
export function diffSnapshots(
  before: Record<string, unknown> | null,
  after: Record<string, unknown> | null,
): FieldChange[] {
  if (!before || !after) return [];
  const keys = new Set([...Object.keys(before), ...Object.keys(after)]);
  const out: FieldChange[] = [];
  for (const k of keys) {
    if (IGNORED.has(k)) continue;
    const b = show(before[k]);
    const a = show(after[k]);
    if (b !== a) out.push({ field: k, before: b, after: a });
  }
  return out;
}
