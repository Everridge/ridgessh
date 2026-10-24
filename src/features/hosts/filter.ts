import type { Group, Host } from "@/lib/ipc";
import type { HostView } from "@/stores/ui";

export interface GroupNode extends Group {
  children: GroupNode[];
  depth: number;
}

/** Builds the group forest, sorted by `sortOrder` then name. Orphans become roots. */
export function buildGroupTree(groups: Group[]): GroupNode[] {
  const byId = new Map(groups.map((g) => [g.id, { ...g, children: [], depth: 0 } as GroupNode]));
  const roots: GroupNode[] = [];
  for (const node of byId.values()) {
    const parent = node.parentId ? byId.get(node.parentId) : undefined;
    (parent ? parent.children : roots).push(node);
  }
  const sort = (nodes: GroupNode[], depth: number) => {
    nodes.sort((a, b) => a.sortOrder - b.sortOrder || a.name.localeCompare(b.name));
    for (const n of nodes) {
      n.depth = depth;
      sort(n.children, depth + 1);
    }
  };
  sort(roots, 0);
  return roots;
}

/** Depth-first list of the tree, handy for `<select>` options. */
export function flattenTree(nodes: GroupNode[]): GroupNode[] {
  return nodes.flatMap((n) => [n, ...flattenTree(n.children)]);
}

/** `id` plus the ids of all its descendants. */
export function descendantIds(groups: Group[], id: string): Set<string> {
  const out = new Set([id]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const g of groups) {
      if (g.parentId && out.has(g.parentId) && !out.has(g.id)) {
        out.add(g.id);
        grew = true;
      }
    }
  }
  return out;
}

/** "Parent / Child" path for a group id. */
export function groupPath(groups: Group[], id: string | null): string {
  const byId = new Map(groups.map((g) => [g.id, g]));
  const parts: string[] = [];
  const seen = new Set<string>();
  let cur = id ? byId.get(id) : undefined;
  while (cur && !seen.has(cur.id)) {
    seen.add(cur.id);
    parts.unshift(cur.name);
    cur = cur.parentId ? byId.get(cur.parentId) : undefined;
  }
  return parts.join(" / ");
}

function searchText(h: Host, groups: Group[]): string {
  return [
    h.alias,
    h.hostname,
    h.user,
    h.port?.toString(),
    h.description,
    h.proxyJump,
    groupPath(groups, h.groupId),
    ...h.tags.map((t) => t.name),
    ...h.fields.map((f) => `${f.key} ${f.value}`),
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
}

export interface HostFilter {
  view: HostView;
  groupId: string | null;
  search: string;
}

/** Applies the sidebar view and the search box. Every search word must match. */
export function filterHosts(hosts: Host[], groups: Group[], f: HostFilter): Host[] {
  const inGroup = f.view === "group" && f.groupId ? descendantIds(groups, f.groupId) : null;
  let out = hosts.filter((h) => {
    if (f.view === "trash") return h.deletedAt !== null;
    if (h.deletedAt !== null) return false;
    switch (f.view) {
      case "favorites":
        return h.favorite;
      case "recent":
        return h.lastConnectedAt !== null;
      case "removed":
        return h.syncState === "removed_from_config";
      case "group":
        return inGroup !== null && h.groupId !== null && inGroup.has(h.groupId);
      default:
        return true;
    }
  });

  const words = f.search.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length) {
    out = out.filter((h) => {
      const text = searchText(h, groups);
      return words.every((w) => text.includes(w));
    });
  }

  if (f.view === "recent") {
    out = [...out].sort((a, b) => (b.lastConnectedAt ?? "").localeCompare(a.lastConnectedAt ?? ""));
  }
  return out;
}

/** Live (non-deleted) host count per group, including hosts in subgroups. */
export function countByGroup(hosts: Host[], groups: Group[]): Map<string, number> {
  const counts = new Map<string, number>();
  const parent = new Map(groups.map((g) => [g.id, g.parentId]));
  for (const h of hosts) {
    if (h.deletedAt !== null) continue;
    const seen = new Set<string>();
    let cur = h.groupId;
    while (cur && !seen.has(cur)) {
      seen.add(cur);
      counts.set(cur, (counts.get(cur) ?? 0) + 1);
      cur = parent.get(cur) ?? null;
    }
  }
  return counts;
}
