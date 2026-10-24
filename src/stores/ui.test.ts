import { beforeEach, describe, expect, it } from "vitest";
import { useUiStore } from "./ui";

describe("ui store", () => {
  beforeEach(() =>
    useUiStore.setState({
      view: "all",
      groupId: null,
      search: "",
      selectedHostId: null,
      checkedIds: [],
    }),
  );

  it("clears selection and checked hosts when switching views", () => {
    const s = useUiStore.getState();
    s.selectHost("abc");
    s.toggleChecked("abc");
    s.setView("trash");
    expect(useUiStore.getState()).toMatchObject({
      view: "trash",
      selectedHostId: null,
      checkedIds: [],
    });
  });

  it("selecting a group switches to group view", () => {
    useUiStore.getState().setGroup("g1");
    expect(useUiStore.getState()).toMatchObject({ view: "group", groupId: "g1" });
    useUiStore.getState().setView("all");
    expect(useUiStore.getState().groupId).toBeNull();
  });

  it("toggles checked ids", () => {
    const s = useUiStore.getState();
    s.toggleChecked("a");
    s.toggleChecked("b");
    s.toggleChecked("a");
    expect(useUiStore.getState().checkedIds).toEqual(["b"]);
  });
});
