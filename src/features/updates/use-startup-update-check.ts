import { useEffect, useRef } from "react";
import { useSettings } from "@/features/backup/queries";
import { useUpdateStore } from "./store";

/**
 * Looks for a new release once, a few seconds after start, when the setting allows it, and
 * offers it in the update dialog unless the user skipped that version. Failures stay quiet:
 * being offline is not worth a notification.
 */
export function useStartupUpdateCheck(enabled = import.meta.env.PROD, delayMs = 4000) {
  const allowed = useSettings().data?.settings.checkUpdates;
  const started = useRef(false);

  useEffect(() => {
    if (!enabled || !allowed || started.current) return;
    // Marked inside the timer: StrictMode's simulated unmount clears a pending one.
    const timer = setTimeout(() => {
      started.current = true;
      useUpdateStore
        .getState()
        .check()
        .then((update) => {
          const store = useUpdateStore.getState();
          if (update && update.version !== store.skipped) store.setDialogOpen(true);
        })
        .catch((e: unknown) => console.warn("update check failed", e));
    }, delayMs);
    return () => clearTimeout(timer);
  }, [enabled, allowed, delayMs]);
}
