import { useEffect, useRef } from "react";
import { toast } from "sonner";
import { useSettings } from "@/features/backup/queries";
import { useUpdateStore } from "./store";

/**
 * Looks for a new release once, a few seconds after start, when the setting allows it.
 * Failures stay quiet: being offline is not worth a notification.
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
          if (!update) return;
          toast.info(`RidgeSSH ${update.version} is available`, {
            action: {
              label: "View",
              onClick: () => useUpdateStore.getState().setDialogOpen(true),
            },
            duration: 15_000,
          });
        })
        .catch((e: unknown) => console.warn("update check failed", e));
    }, delayMs);
    return () => clearTimeout(timer);
  }, [enabled, allowed, delayMs]);
}
