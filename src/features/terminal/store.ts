import { create } from "zustand";

export type TabStatus = "connecting" | "open" | "exited" | "error";

export interface TerminalTab {
  id: string;
  hostId: string;
  title: string;
  status: TabStatus;
  exitCode: number | null;
  /** Bumped to reconnect: the view opens a new session but keeps its scrollback. */
  generation: number;
}

interface TerminalState {
  tabs: TerminalTab[];
  activeId: string | null;
  open: (host: { id: string; alias: string }) => string;
  close: (id: string) => void;
  activate: (id: string) => void;
  setStatus: (id: string, status: TabStatus, exitCode?: number | null) => void;
  reconnect: (id: string) => void;
}

let seq = 0;

/** Open terminal tabs. Sessions themselves live in the Rust backend. */
export const useTerminalStore = create<TerminalState>((set, get) => ({
  tabs: [],
  activeId: null,
  open: (host) => {
    const id = `tab-${++seq}`;
    const same = get().tabs.filter((t) => t.hostId === host.id).length;
    const title = same ? `${host.alias} (${same + 1})` : host.alias;
    set((s) => ({
      tabs: [
        ...s.tabs,
        { id, hostId: host.id, title, status: "connecting", exitCode: null, generation: 0 },
      ],
      activeId: id,
    }));
    return id;
  },
  close: (id) =>
    set((s) => {
      const i = s.tabs.findIndex((t) => t.id === id);
      const tabs = s.tabs.filter((t) => t.id !== id);
      const activeId =
        s.activeId === id ? (tabs[Math.min(i, tabs.length - 1)]?.id ?? null) : s.activeId;
      return { tabs, activeId };
    }),
  activate: (activeId) => set({ activeId }),
  setStatus: (id, status, exitCode = null) =>
    set((s) => ({ tabs: s.tabs.map((t) => (t.id === id ? { ...t, status, exitCode } : t)) })),
  reconnect: (id) =>
    set((s) => ({
      tabs: s.tabs.map((t) =>
        t.id === id
          ? { ...t, status: "connecting", exitCode: null, generation: t.generation + 1 }
          : t,
      ),
    })),
}));
