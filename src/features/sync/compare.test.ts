import { describe, expect, it } from "vitest";
import { makeHost } from "@/test/fake-backend";
import { compareWithConfig, reviewItems, summarize } from "./compare";

describe("compareWithConfig", () => {
  it("marks differing fields between DB and config", () => {
    const h = makeHost({
      alias: "db",
      hostname: "10.0.0.5",
      port: 2222,
      extraOptions: [{ key: "ForwardAgent", value: "yes" }],
      configSnapshot: {
        alias: "db",
        hostname: "10.0.0.5",
        user: null,
        port: null,
        identityFile: null,
        proxyJump: null,
        extraOptions: [],
        userMatches: [],
      },
    });
    const diff = compareWithConfig(h).filter((r) => r.differs);
    expect(diff).toEqual([
      { label: "Port", db: "2222", config: "—", differs: true },
      { label: "Other options", db: "ForwardAgent yes", config: "—", differs: true },
    ]);
  });

  it("is empty for hosts not in the config", () => {
    expect(compareWithConfig(makeHost())).toEqual([]);
  });
});

describe("review helpers", () => {
  it("groups hosts by what needs review and ignores trash", () => {
    const items = reviewItems([
      makeHost({ id: "1", syncState: "config_only_new" }),
      makeHost({ id: "2", syncState: "removed_from_config" }),
      makeHost({ id: "3", syncState: "conflict" }),
      makeHost({ id: "4", syncState: "config_only_new", deletedAt: "x" }),
      makeHost({ id: "5", syncState: "synced" }),
    ]);
    expect(items.added.map((h) => h.id)).toEqual(["1"]);
    expect(items.removed.map((h) => h.id)).toEqual(["2"]);
    expect(items.conflicts.map((h) => h.id)).toEqual(["3"]);
  });

  it("summarizes a report", () => {
    expect(
      summarize({
        added: ["a", "b"],
        updated: [],
        removed: ["c"],
        reappeared: [],
        conflicts: ["d"],
      }),
    ).toBe("2 new, 1 removed, 1 conflict");
    expect(summarize({ added: [], updated: [], removed: [], reappeared: [], conflicts: [] })).toBe(
      "",
    );
  });
});
