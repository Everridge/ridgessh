import { beforeEach, describe, expect, it } from "vitest";
import { useUiStore } from "./ui";

describe("ui store", () => {
  beforeEach(() => useUiStore.setState({ view: "all", search: "", selectedHostId: null }));

  it("clears the selection when switching views", () => {
    useUiStore.getState().selectHost("abc");
    useUiStore.getState().setView("trash");
    expect(useUiStore.getState()).toMatchObject({ view: "trash", selectedHostId: null });
  });
});
