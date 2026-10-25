import type { Connection, Host, UserMatch } from "@/lib/ipc";

export interface ConnectionRow {
  label: string;
  db: string;
  config: string;
  differs: boolean;
}

const FIELDS: { key: keyof Omit<Connection, "extraOptions" | "userMatches">; label: string }[] = [
  { key: "alias", label: "Host" },
  { key: "hostname", label: "HostName" },
  { key: "user", label: "User" },
  { key: "port", label: "Port" },
  { key: "identityFile", label: "IdentityFile" },
  { key: "proxyJump", label: "ProxyJump" },
];

const show = (v: unknown) => (v === null || v === undefined || v === "" ? "—" : String(v));
const options = (c: Connection) =>
  c.extraOptions.map((o) => `${o.key} ${o.value}`).join("\n") || "—";

/** One line per user: `user  IdentityFile key · other options`. */
const users = (ms: UserMatch[]) =>
  ms
    .map((m) =>
      [
        m.user,
        [
          ...(m.identityFile ? [`IdentityFile ${m.identityFile}`] : []),
          ...m.extraOptions.map((o) => `${o.key} ${o.value}`),
        ].join(" · "),
      ]
        .filter(Boolean)
        .join("  "),
    )
    .join("\n") || "—";

/** Side-by-side rows of a host's DB connection vs. what the config says. */
export function compareWithConfig(h: Host): ConnectionRow[] {
  const cfg = h.configSnapshot;
  if (!cfg) return [];
  const rows = FIELDS.map(({ key, label }) => {
    const db = show(h[key]);
    const config = show(cfg[key]);
    return { label, db, config, differs: db !== config };
  });
  const dbOpts = options({ ...cfg, extraOptions: h.extraOptions });
  const cfgOpts = options(cfg);
  rows.push({ label: "Other options", db: dbOpts, config: cfgOpts, differs: dbOpts !== cfgOpts });
  const dbUsers = users(h.userMatches);
  const cfgUsers = users(cfg.userMatches ?? []);
  rows.push({ label: "Users", db: dbUsers, config: cfgUsers, differs: dbUsers !== cfgUsers });
  return rows;
}

/** Hosts needing a decision after a scan. */
export function reviewItems(hosts: Host[]) {
  const live = hosts.filter((h) => h.deletedAt === null);
  return {
    added: live.filter((h) => h.syncState === "config_only_new"),
    removed: live.filter((h) => h.syncState === "removed_from_config"),
    conflicts: live.filter((h) => h.syncState === "conflict"),
  };
}

export function summarize(r: {
  added: string[];
  updated: string[];
  removed: string[];
  reappeared: string[];
  conflicts: string[];
}): string {
  const parts = [
    [r.added.length, "new"],
    [r.updated.length, "updated"],
    [r.reappeared.length, "back in config"],
    [r.removed.length, "removed"],
    [r.conflicts.length, r.conflicts.length === 1 ? "conflict" : "conflicts"],
  ] as const;
  return parts
    .filter(([n]) => n > 0)
    .map(([n, label]) => `${n} ${label}`)
    .join(", ");
}
