import { useEffect } from "react";
import { useUiStore } from "@/stores/ui";

/** Ctrl+K anywhere except inside a terminal (where Ctrl+K is the shell's); Ctrl+Shift+K always. */
export function useQuickConnectShortcut() {
  const setOpen = useUiStore((s) => s.setQuickConnectOpen);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || e.altKey || e.code !== "KeyK") return;
      const inTerminal = (document.activeElement as HTMLElement | null)?.closest("[data-terminal]");
      if (e.shiftKey || !inTerminal) {
        e.preventDefault();
        setOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setOpen]);
}
