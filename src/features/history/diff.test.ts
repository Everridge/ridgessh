import { describe, expect, it } from "vitest";
import { diffSnapshots } from "./diff";

describe("diffSnapshots", () => {
  it("lists changed fields and ignores bookkeeping columns", () => {
    const before = { alias: "a", port: 22, updatedAt: "1", tags: [{ name: "x" }], favorite: false };
    const after = {
      alias: "a",
      port: 2222,
      updatedAt: "2",
      tags: [{ name: "x" }, { name: "y" }],
      favorite: true,
    };
    expect(diffSnapshots(before, after)).toEqual([
      { field: "port", before: "22", after: "2222" },
      { field: "tags", before: "x", after: "x, y" },
      { field: "favorite", before: "no", after: "yes" },
    ]);
  });

  it("treats null, undefined and empty string as equal", () => {
    expect(diffSnapshots({ user: null }, { user: "" })).toEqual([]);
  });

  it("returns nothing for create/purge entries", () => {
    expect(diffSnapshots(null, { alias: "a" })).toEqual([]);
  });
});
