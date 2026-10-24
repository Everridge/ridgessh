import { beforeEach, describe, expect, it } from "vitest";
import { parseProgress, useTerminalStore } from "./store";

const s = () => useTerminalStore.getState();

describe("terminal store", () => {
  beforeEach(() => useTerminalStore.setState({ tabs: [], activeId: null }));

  it("numbers repeated tabs for the same host and activates new tabs", () => {
    const a = s().open({ id: "h1", alias: "web" });
    const b = s().open({ id: "h1", alias: "web" });
    expect(s().tabs.map((t) => t.title)).toEqual(["web", "web (2)"]);
    expect(s().activeId).toBe(b);
    s().activate(a);
    expect(s().activeId).toBe(a);
  });

  it("activates a neighbour when the active tab closes", () => {
    const a = s().open({ id: "1", alias: "a" });
    const b = s().open({ id: "2", alias: "b" });
    const c = s().open({ id: "3", alias: "c" });
    s().activate(b);
    s().close(b);
    expect(s().activeId).toBe(c);
    s().close(c);
    expect(s().activeId).toBe(a);
    s().close(a);
    expect(s().activeId).toBeNull();
  });

  it("reconnect bumps the generation and resets status", () => {
    const a = s().open({ id: "1", alias: "a" });
    s().setStatus(a, "exited", 255);
    s().reconnect(a);
    expect(s().tabs[0]).toMatchObject({ status: "connecting", exitCode: null, generation: 1 });
  });
});

describe("scp progress", () => {
  it("takes the last percentage in a chunk", () => {
    expect(parseProgress("a.txt   12%  1KB\ra.txt   57%  4KB")).toBe(57);
    expect(parseProgress("no progress here")).toBeNull();
    expect(parseProgress("weird 250%")).toBeNull();
    expect(parseProgress("a.txt 100% 4KB 1.0MB/s 00:00")).toBe(100);
  });
});
