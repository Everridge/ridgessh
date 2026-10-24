import { useEffect } from "react";
import { isMac, modKey } from "@/lib/platform";
import { useUiStore } from "@/stores/ui";

/**
 * Ctrl+K anywhere except inside a terminal (where Ctrl+K is the shell's); Ctrl+Shift+K always.
 * On macOS it is ⌘K, which never reaches the shell, so it works everywhere.
 */
export function useQuickConnectShortcut() {
  const setOpen = useUiStore((s) => s.setQuickConnectOpen);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!modKey(e) || e.altKey || e.code !== "KeyK") return;
      const inTerminal = (document.activeElement as HTMLElement | null)?.closest("[data-terminal]");
      if (isMac || e.shiftKey || !inTerminal) {
        e.preventDefault();
        setOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setOpen]);
}
