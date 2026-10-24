import { create } from "zustand";

export type HostView = "all" | "favorites" | "recent" | "removed" | "trash" | "group";

interface UiState {
  view: HostView;
  /** Selected group when `view === "group"`. */
  groupId: string | null;
  search: string;
  selectedHostId: string | null;
  /** Hosts ticked for bulk actions. */
  checkedIds: string[];
  /** Host form dialog: `null` closed, `"new"` create, otherwise the host id being edited. */
  hostForm: "new" | string | null;
  /** Config sync review dialog. */
  syncReviewOpen: boolean;
  quickConnectOpen: boolean;
  setView: (view: Exclude<HostView, "group">) => void;
  setGroup: (groupId: string) => void;
  setSearch: (search: string) => void;
  selectHost: (id: string | null) => void;
  setChecked: (ids: string[]) => void;
  toggleChecked: (id: string) => void;
  openHostForm: (target: "new" | string) => void;
  closeHostForm: () => void;
  setSyncReviewOpen: (open: boolean) => void;
  setQuickConnectOpen: (open: boolean) => void;
}

/** Pure UI state. Backend data lives in TanStack Query, not here. */
export const useUiStore = create<UiState>((set) => ({
  view: "all",
  groupId: null,
  search: "",
  selectedHostId: null,
  checkedIds: [],
  hostForm: null,
  syncReviewOpen: false,
  quickConnectOpen: false,
  setView: (view) => set({ view, groupId: null, selectedHostId: null, checkedIds: [] }),
  setGroup: (groupId) => set({ view: "group", groupId, selectedHostId: null, checkedIds: [] }),
  setSearch: (search) => set({ search }),
  selectHost: (selectedHostId) => set({ selectedHostId }),
  setChecked: (checkedIds) => set({ checkedIds }),
  toggleChecked: (id) =>
    set((s) => ({
      checkedIds: s.checkedIds.includes(id)
        ? s.checkedIds.filter((x) => x !== id)
        : [...s.checkedIds, id],
    })),
  openHostForm: (hostForm) => set({ hostForm }),
  closeHostForm: () => set({ hostForm: null }),
  setSyncReviewOpen: (syncReviewOpen) => set({ syncReviewOpen }),
  setQuickConnectOpen: (quickConnectOpen) => set({ quickConnectOpen }),
}));
