import { create } from "zustand";

export type HostView = "all" | "favorites" | "recent" | "removed" | "trash" | "group";

interface UiState {
  view: HostView;
  /** Selected group when `view === "group"`. */
  groupId: string | null;
  historyOpen: boolean;
  /** Upload / download dialog for one host. */
  transfer: { hostId: string; direction: "upload" | "download" } | null;
  scriptsOpen: boolean;
  /** Run-script dialog: target hosts and an optional pre-selected script. */
  scriptRun: { hostIds: string[]; scriptId?: string } | null;
  settingsOpen: boolean;
  backupsOpen: boolean;
  /** Export dialog; `hostIds` pre-selects those hosts (`null` = all). */
  exportTarget: { hostIds: string[] | null } | null;
  importOpen: boolean;
  /** Backup file being restored (from the list or picked from disk). */
  restoreTarget: { path: string; name: string } | null;
  search: string;
  selectedHostId: string | null;
  /** Hosts ticked for bulk actions. */
  checkedIds: string[];
  /** Host form dialog: `null` closed, `"new"` create, otherwise the host id being edited. */
  hostForm: "new" | string | null;
  /** Config sync review dialog. */
  syncReviewOpen: boolean;
  quickConnectOpen: boolean;
  /** Write-to-config dialog: `null` closed; `focus` pre-selects only those hosts. */
  configWrite: { focus: string[] | null } | null;
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
  openConfigWrite: (focus?: string[]) => void;
  closeConfigWrite: () => void;
  setHistoryOpen: (open: boolean) => void;
  openTransfer: (hostId: string, direction: "upload" | "download") => void;
  closeTransfer: () => void;
  setScriptsOpen: (open: boolean) => void;
  openScriptRun: (hostIds: string[], scriptId?: string) => void;
  closeScriptRun: () => void;
  setSettingsOpen: (open: boolean) => void;
  setBackupsOpen: (open: boolean) => void;
  setRestoreTarget: (target: { path: string; name: string } | null) => void;
  openExport: (hostIds?: string[]) => void;
  closeExport: () => void;
  setImportOpen: (open: boolean) => void;
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
  configWrite: null,
  historyOpen: false,
  transfer: null,
  scriptsOpen: false,
  scriptRun: null,
  settingsOpen: false,
  backupsOpen: false,
  restoreTarget: null,
  exportTarget: null,
  importOpen: false,
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
  openConfigWrite: (focus) => set({ configWrite: { focus: focus ?? null } }),
  closeConfigWrite: () => set({ configWrite: null }),
  setHistoryOpen: (historyOpen) => set({ historyOpen }),
  openTransfer: (hostId, direction) => set({ transfer: { hostId, direction } }),
  closeTransfer: () => set({ transfer: null }),
  setScriptsOpen: (scriptsOpen) => set({ scriptsOpen }),
  openScriptRun: (hostIds, scriptId) => set({ scriptRun: { hostIds, scriptId } }),
  closeScriptRun: () => set({ scriptRun: null }),
  setSettingsOpen: (settingsOpen) => set({ settingsOpen }),
  setBackupsOpen: (backupsOpen) => set({ backupsOpen }),
  setRestoreTarget: (restoreTarget) => set({ restoreTarget }),
  openExport: (hostIds) => set({ exportTarget: { hostIds: hostIds ?? null } }),
  closeExport: () => set({ exportTarget: null }),
  setImportOpen: (importOpen) => set({ importOpen }),
}));
