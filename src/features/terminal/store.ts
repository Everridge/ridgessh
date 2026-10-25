import { create } from "zustand";
import type { FolderSpec, ScriptRun, StepStatus, TransferSpec } from "@/lib/ipc";

export type TabStatus = "connecting" | "open" | "exited" | "error";

/** What a tab runs: an interactive session, a file transfer or a script. */
export type TabJob =
  /** `user`: connect as `user@alias` (a per-user `Match` block). */
  | { kind: "ssh"; user?: string | null }
  | { kind: "transfer"; spec: TransferSpec }
  | { kind: "script"; run: ScriptRun }
  | { kind: "folder"; spec: FolderSpec };

export interface FolderStep {
  label: string;
  status: StepStatus;
  detail: string | null;
}

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
  /** Folder transfers: the backend's run (for Retry) and its steps. */
  run?: { id: string; steps: FolderStep[] } | null;
}

/** The run to continue: one whose last attempt stopped before every step finished. */
export function resumableRun(tab: TerminalTab): string | null {
  const steps = tab.run?.steps ?? [];
  const unfinished = steps.some((s) => !["done", "warning", "skipped"].includes(s.status));
  return tab.run && unfinished ? tab.run.id : null;
}

interface TerminalState {
  tabs: TerminalTab[];
  activeId: string | null;
  open: (host: { id: string; alias: string }, user?: string | null) => string;
  openJob: (host: { id: string; alias: string }, job: TabJob, title: string) => string;
  close: (id: string) => void;
  activate: (id: string) => void;
  setStatus: (id: string, status: TabStatus, exitCode?: number | null) => void;
  setProgress: (id: string, progress: number) => void;
  reconnect: (id: string) => void;
  stop: (id: string) => void;
  setRun: (id: string, runId: string, labels: string[]) => void;
  setStep: (id: string, index: number, status: StepStatus, detail: string | null) => void;
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
    open: (host, user = null) => {
      const name = user ? `${user}@${host.alias}` : host.alias;
      const same = get().tabs.filter(
        (t) => t.hostId === host.id && t.job.kind === "ssh" && (t.job.user ?? null) === user,
      ).length;
      return add(host.id, same ? `${name} (${same + 1})` : name, { kind: "ssh", user });
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
    setRun: (id, runId, labels) =>
      set((s) => ({
        tabs: s.tabs.map((t) =>
          t.id === id
            ? {
                ...t,
                run: {
                  id: runId,
                  steps: labels.map((label) => ({ label, status: "pending", detail: null })),
                },
              }
            : t,
        ),
      })),
    setStep: (id, index, status, detail) =>
      set((s) => ({
        tabs: s.tabs.map((t) =>
          t.id === id && t.run
            ? {
                ...t,
                run: {
                  ...t.run,
                  steps: t.run.steps.map((st, i) => (i === index ? { ...st, status, detail } : st)),
                },
              }
            : t,
        ),
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
