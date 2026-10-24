import { create } from "zustand";
import type { ScriptRun, TransferSpec } from "@/lib/ipc";

export type TabStatus = "connecting" | "open" | "exited" | "error";

/** What a tab runs: an interactive session, a file transfer or a script. */
export type TabJob =
  { kind: "ssh" } | { kind: "transfer"; spec: TransferSpec } | { kind: "script"; run: ScriptRun };

export interface TerminalTab {
  id: string;
  hostId: string;
  title: string;
  job: TabJob;
  status: TabStatus;
  exitCode: number | null;
  /** Transfers: last percentage scp reported. */
  progress: number | null;
  /** Bumped to reconnect / run again: the view starts a new process, keeping scrollback. */
  generation: number;
  /** Bumped to stop the running process (output stays in the tab). */
  stopSignal: number;
}

interface TerminalState {
  tabs: TerminalTab[];
  activeId: string | null;
  open: (host: { id: string; alias: string }) => string;
  openJob: (host: { id: string; alias: string }, job: TabJob, title: string) => string;
  close: (id: string) => void;
  activate: (id: string) => void;
  setStatus: (id: string, status: TabStatus, exitCode?: number | null) => void;
  setProgress: (id: string, progress: number) => void;
  reconnect: (id: string) => void;
  stop: (id: string) => void;
}

let seq = 0;

/** Open terminal tabs. Processes themselves live in the Rust backend. */
export const useTerminalStore = create<TerminalState>((set, get) => {
  const add = (hostId: string, title: string, job: TabJob) => {
    const id = `tab-${++seq}`;
    set((s) => ({
      tabs: [
        ...s.tabs,
        {
          id,
          hostId,
          title,
          job,
          status: "connecting",
          exitCode: null,
          progress: null,
          generation: 0,
          stopSignal: 0,
        },
      ],
      activeId: id,
    }));
    return id;
  };
  return {
    tabs: [],
    activeId: null,
    open: (host) => {
      const same = get().tabs.filter((t) => t.hostId === host.id && t.job.kind === "ssh").length;
      return add(host.id, same ? `${host.alias} (${same + 1})` : host.alias, { kind: "ssh" });
    },
    openJob: (host, job, title) => add(host.id, title, job),
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
    setProgress: (id, progress) =>
      set((s) => ({
        tabs: s.tabs.map((t) => (t.id === id && t.progress !== progress ? { ...t, progress } : t)),
      })),
    stop: (id) =>
      set((s) => ({
        tabs: s.tabs.map((t) => (t.id === id ? { ...t, stopSignal: t.stopSignal + 1 } : t)),
      })),
    reconnect: (id) =>
      set((s) => ({
        tabs: s.tabs.map((t) =>
          t.id === id
            ? {
                ...t,
                status: "connecting",
                exitCode: null,
                progress: null,
                generation: t.generation + 1,
              }
            : t,
        ),
      })),
  };
});

/** Last "NN%" in a chunk of scp output (its progress meter), or null. */
export function parseProgress(text: string): number | null {
  const all = [...text.matchAll(/(\d{1,3})%/g)];
  const last = all.at(-1);
  if (!last) return null;
  const n = Number(last[1]);
  return n <= 100 ? n : null;
}
