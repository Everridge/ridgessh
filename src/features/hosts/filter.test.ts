import { describe, expect, it } from "vitest";
import type { Group, Host } from "@/lib/ipc";
import { makeHost } from "@/test/fake-backend";
import { buildGroupTree, countByGroup, filterHosts, groupByServer, groupPath } from "./filter";

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
  configSnapshot: null,
  configSource: null,
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

describe("groupByServer", () => {
  const h = (alias: string, hostname: string | null, port: number | null = null) =>
    makeHost({ id: alias, alias, hostname, port });

  it("nests hosts sharing hostname and port, in list order", () => {
    const list = [
      h("acme_prod_deploy", "203.0.113.20"),
      h("web", "10.0.0.1"),
      h("acme_prod_root", "203.0.113.20", 22),
      h("ACME_PROD_tom", "203.0.113.20"),
      h("alt-port", "203.0.113.20", 2222),
      h("no-hostname", null),
      h("node1", "203.0.113.30"),
      h("node1-web", "203.0.113.30"),
    ];
    const out = groupByServer(list);
    expect(
      out.map((e) =>
        e.kind === "host" ? e.host.alias : `${e.name} [${e.hosts.map((x) => x.alias).join(",")}]`,
      ),
    ).toEqual([
      "203.0.113.20 [acme_prod_deploy,acme_prod_root,ACME_PROD_tom]",
      "web",
      "alt-port",
      "no-hostname",
      "node1 [node1,node1-web]",
    ]);
  });

  it("names a server by the aliases' common prefix", () => {
    const out = groupByServer([h("acme_prod_root", "1.2.3.4"), h("acme_prod_tom", "1.2.3.4")]);
    expect(out[0]).toMatchObject({ kind: "server", name: "acme_prod", address: "1.2.3.4" });
    const port = groupByServer([h("a", "x.example", 2222), h("b", "X.example", 2222)]);
    expect(port[0]).toMatchObject({ name: "x.example:2222", address: "x.example:2222" });
  });
});
