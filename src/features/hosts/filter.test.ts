import { describe, expect, it } from "vitest";
import type { Group, Host } from "@/lib/ipc";
import { buildGroupTree, countByGroup, filterHosts, groupPath } from "./filter";

const group = (id: string, name: string, parentId: string | null = null): Group => ({
  id,
  name,
  parentId,
  color: null,
  icon: null,
  sortOrder: 0,
  createdAt: "",
  updatedAt: "",
});

const host = (alias: string, over: Partial<Host> = {}): Host => ({
  id: alias,
  alias,
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
  syncState: "synced",
  configDirty: false,
  lastSeenInConfigAt: null,
  removedFromConfigAt: null,
  lastConnectedAt: null,
  deletedAt: null,
  createdAt: "",
  updatedAt: "",
  tags: [],
  fields: [],
  ...over,
});

const groups = [group("prod", "Prod"), group("db", "Databases", "prod"), group("dev", "Dev")];
const hosts = [
  host("pg1", {
    groupId: "db",
    hostname: "10.0.0.1",
    tags: [{ id: "t", name: "postgres", color: null }],
  }),
  host("web1", { groupId: "prod", favorite: true, user: "deploy" }),
  host("dev1", { groupId: "dev", syncState: "removed_from_config" }),
  host("old", { deletedAt: "2026-01-01T00:00:00Z" }),
];

const aliases = (hs: Host[]) => hs.map((h) => h.alias);

describe("filterHosts", () => {
  it("hides trashed hosts except in trash", () => {
    expect(aliases(filterHosts(hosts, groups, { view: "all", groupId: null, search: "" }))).toEqual(
      ["pg1", "web1", "dev1"],
    );
    expect(
      aliases(filterHosts(hosts, groups, { view: "trash", groupId: null, search: "" })),
    ).toEqual(["old"]);
  });

  it("includes subgroups in group view", () => {
    expect(
      aliases(filterHosts(hosts, groups, { view: "group", groupId: "prod", search: "" })),
    ).toEqual(["pg1", "web1"]);
  });

  it("matches every search word across fields, tags and group path", () => {
    const f = (search: string) =>
      aliases(filterHosts(hosts, groups, { view: "all", groupId: null, search }));
    expect(f("postgres prod")).toEqual(["pg1"]);
    expect(f("DEPLOY")).toEqual(["web1"]);
    expect(f("databases 10.0")).toEqual(["pg1"]);
    expect(f("nothing")).toEqual([]);
  });

  it("filters favorites and removed-from-config", () => {
    expect(
      aliases(filterHosts(hosts, groups, { view: "favorites", groupId: null, search: "" })),
    ).toEqual(["web1"]);
    expect(
      aliases(filterHosts(hosts, groups, { view: "removed", groupId: null, search: "" })),
    ).toEqual(["dev1"]);
  });
});

describe("groups", () => {
  it("builds a tree with depth", () => {
    const tree = buildGroupTree(groups);
    expect(tree.map((n) => n.name)).toEqual(["Dev", "Prod"]);
    expect(tree[1].children[0]).toMatchObject({ name: "Databases", depth: 1 });
  });

  it("builds group paths", () => {
    expect(groupPath(groups, "db")).toBe("Prod / Databases");
    expect(groupPath(groups, null)).toBe("");
  });

  it("counts hosts including subgroups, ignoring trash", () => {
    const c = countByGroup(hosts, groups);
    expect(c.get("prod")).toBe(2);
    expect(c.get("db")).toBe(1);
    expect(c.get("dev")).toBe(1);
  });
});
