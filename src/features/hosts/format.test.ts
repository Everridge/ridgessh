import { describe, expect, it } from "vitest";
import { duration } from "./format";

describe("duration", () => {
  it("formats seconds, minutes and hours", () => {
    const t = (s: number) => new Date(Date.UTC(2026, 0, 1) + s * 1000).toISOString();
    expect(duration(t(0), t(35))).toBe("35s");
    expect(duration(t(0), t(125))).toBe("2m 5s");
    expect(duration(t(0), t(3840))).toBe("1h 4m");
    expect(duration(t(0), null)).toBe("running");
  });
});
