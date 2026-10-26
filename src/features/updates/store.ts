import { create } from "zustand";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { errorMessage } from "@/lib/ipc";

export type UpdateStatus =
  "idle" | "checking" | "none" | "available" | "downloading" | "installing" | "error";

interface UpdateState {
  status: UpdateStatus;
  update: Update | null;
  /** Bytes downloaded so far and the total, when the server sends one. */
  downloaded: number;
  total: number | null;
  error: string | null;
  dialogOpen: boolean;
  /** Asks the release server; returns the update when one is newer than this build. */
  check: () => Promise<Update | null>;
  /** Downloads, installs and restarts. On Windows the installer closes the app itself. */
  install: () => Promise<void>;
  setDialogOpen: (open: boolean) => void;
  /** Version the user chose to skip: the startup check stays quiet about it. */
  skipped: string | null;
  skip: (version: string) => void;
}

const SKIPPED_KEY = "ridgessh.updates.skipped";

function readSkipped() {
  try {
    return localStorage.getItem(SKIPPED_KEY);
  } catch {
    return null;
  }
}

/** The updater's own errors name a URL or JSON; say what it means for the user instead. */
function checkError(e: unknown) {
  const message = errorMessage(e);
  return /fetch|json|request|connect|network|unreachable|dns|timed out|404/i.test(message)
    ? `Could not reach the update server. Check your internet connection and try again. (${message})`
    : message;
}

export const useUpdateStore = create<UpdateState>((set, get) => ({
  status: "idle",
  update: null,
  downloaded: 0,
  total: null,
  error: null,
  dialogOpen: false,
  skipped: readSkipped(),

  check: async () => {
    const busy = get().status;
    if (busy === "checking" || busy === "downloading" || busy === "installing") {
      return get().update;
    }
    set({ status: "checking", error: null });
    try {
      const update = await check();
      set({ status: update ? "available" : "none", update });
      return update;
    } catch (e) {
      set({ status: "error", error: checkError(e) });
      throw e;
    }
  },

  install: async () => {
    const { update } = get();
    if (!update) return;
    set({ status: "downloading", downloaded: 0, total: null, error: null });
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") set({ total: event.data.contentLength ?? null });
        else if (event.event === "Progress")
          set((s) => ({ downloaded: s.downloaded + event.data.chunkLength }));
        else set({ status: "installing" });
      });
      await relaunch();
    } catch (e) {
      set({ status: "error", error: errorMessage(e) });
    }
  },

  setDialogOpen: (dialogOpen) => set({ dialogOpen }),

  skip: (version) => {
    try {
      localStorage.setItem(SKIPPED_KEY, version);
    } catch {
      // Not remembered across restarts; still closes for now.
    }
    set({ skipped: version, dialogOpen: false });
  },
}));
