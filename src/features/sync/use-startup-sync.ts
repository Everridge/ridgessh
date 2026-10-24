import { useEffect, useRef } from "react";
import { toast } from "sonner";
import { useUiStore } from "@/stores/ui";
import { summarize } from "./compare";
import { useRunSync } from "./queries";

/** Scans ~/.ssh/config once when the app opens and reports what changed. */
export function useStartupSync() {
  const { mutate } = useRunSync();
  const openReview = useUiStore((s) => s.setSyncReviewOpen);
  // A ref survives StrictMode's simulated remount, so the scan runs once per app start.
  const started = useRef(false);

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    mutate(undefined, {
      onSuccess: (report) => {
        const text = summarize(report);
        if (!text) return;
        toast.info(`SSH config scanned: ${text}`, {
          action: { label: "Review", onClick: () => openReview(true) },
          duration: 10_000,
        });
      },
    });
  }, [mutate, openReview]);
}
