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
}

export const useUpdateStore = create<UpdateState>((set, get) => ({
  status: "idle",
  update: null,
  downloaded: 0,
  total: null,
  error: null,
  dialogOpen: false,

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
      set({ status: "error", error: errorMessage(e) });
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
}));
