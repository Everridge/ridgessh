import { create } from "zustand";

export type HostView = "all" | "favorites" | "recent" | "removed" | "trash";

interface UiState {
  view: HostView;
  search: string;
  selectedHostId: string | null;
  setView: (view: HostView) => void;
  setSearch: (search: string) => void;
  selectHost: (id: string | null) => void;
}

/** Pure UI state. Backend data lives in TanStack Query, not here. */
export const useUiStore = create<UiState>((set) => ({
  view: "all",
  search: "",
  selectedHostId: null,
  setView: (view) => set({ view, selectedHostId: null }),
  setSearch: (search) => set({ search }),
  selectHost: (selectedHostId) => set({ selectedHostId }),
}));
