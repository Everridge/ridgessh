import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal, type ITheme } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useTheme } from "@/components/theme-provider";
import { invalidateAll } from "@/features/hosts/queries";
import { api, errorMessage, type TerminalExit } from "@/lib/ipc";
import { parseProgress, useTerminalStore, type TerminalTab } from "./store";

const DARK: ITheme = {
  background: "#0a0a0a",
  foreground: "#e5e5e5",
  cursor: "#e5e5e5",
  selectionBackground: "#3b82f680",
};
const LIGHT: ITheme = {
  background: "#ffffff",
  foreground: "#171717",
  cursor: "#171717",
  selectionBackground: "#3b82f640",
  // Bright white/yellow are unreadable on white; darken them.
  white: "#737373",
  brightWhite: "#525252",
  yellow: "#a16207",
  brightYellow: "#854d0e",
};

const dim = (s: string) => `\x1b[90m${s}\x1b[0m`;
const red = (s: string) => `\x1b[31m${s}\x1b[0m`;

export function TerminalView({ tab, active }: { tab: TerminalTab; active: boolean }) {
  const container = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  const fit = useRef<FitAddon | null>(null);
  const session = useRef<string | null>(null);
  /** Input typed (or terminal replies produced) before the session id is known. */
  const pending = useRef<string[]>([]);
  const { resolvedTheme } = useTheme();
  const qc = useQueryClient();

  // One xterm instance per tab, kept across reconnects so scrollback survives.
  useEffect(() => {
    const t = new Terminal({
      fontFamily: '"Cascadia Mono", "Cascadia Code", Consolas, monospace',
      fontSize: 13,
      cursorBlink: true,
      scrollback: 10_000,
      theme: document.documentElement.classList.contains("dark") ? DARK : LIGHT,
    });
    const f = new FitAddon();
    t.loadAddon(f);
    t.loadAddon(new WebLinksAddon((_, uri) => void openUrl(uri)));
    t.open(container.current!);
    term.current = t;
    fit.current = f;
    f.fit();

    const send = (data: string) => {
      if (session.current) {
        void api.terminalWrite(session.current, data).catch(() => {});
      } else if (
        useTerminalStore.getState().tabs.find((x) => x.id === tab.id)?.status === "connecting"
      ) {
        pending.current.push(data);
      } else if (data === "\r" && tab.job.kind === "ssh") {
        // Only sessions reconnect on Enter; transfers and scripts re-run via the button.
        useTerminalStore.getState().reconnect(tab.id);
      }
    };
    t.onData(send);
    t.onBinary(send);
    t.onResize(({ cols, rows }) => {
      if (session.current) void api.terminalResize(session.current, cols, rows).catch(() => {});
    });

    const copy = () => {
      if (!t.hasSelection()) return false;
      void writeText(t.getSelection());
      t.clearSelection();
      return true;
    };
    const paste = () => void readText().then((text) => text && t.paste(text));
    t.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown" || !e.ctrlKey) return true;
      // Leave app shortcuts (quick connect) to the window.
      if (e.shiftKey && e.code === "KeyK") return false;
      if (e.code === "KeyC" && (e.shiftKey || t.hasSelection())) {
        copy();
        return false;
      }
      if (e.code === "KeyV") {
        e.preventDefault();
        paste();
        return false;
      }
      return true;
    });
    // Windows Terminal style: right click copies the selection, or pastes.
    const onContextMenu = (e: MouseEvent) => {
      e.preventDefault();
      if (!copy()) paste();
    };
    const el = container.current!;
    el.addEventListener("contextmenu", onContextMenu);

    let frame = 0;
    const ro = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if (el.offsetParent !== null) f.fit();
      });
    });
    ro.observe(el);

    return () => {
      ro.disconnect();
      cancelAnimationFrame(frame);
      el.removeEventListener("contextmenu", onContextMenu);
      if (session.current) void api.terminalClose(session.current).catch(() => {});
      session.current = null;
      t.dispose();
      term.current = null;
    };
  }, [tab.id, tab.job.kind]); // the kind never changes for a tab

  // A session per generation (first connect and every reconnect).
  useEffect(() => {
    const t = term.current!;
    const { setStatus, setProgress } = useTerminalStore.getState();
    const job = tab.job;
    let disposed = false;
    pending.current = [];

    const decoder = new TextDecoder();
    const onData = new Channel<ArrayBuffer>();
    onData.onmessage = (buf) => {
      const bytes = new Uint8Array(buf);
      t.write(bytes);
      if (job.kind === "transfer") {
        const pct = parseProgress(decoder.decode(bytes, { stream: true }));
        if (pct !== null) setProgress(tab.id, pct);
      }
    };
    const onExit = new Channel<TerminalExit>();
    onExit.onmessage = ({ exitCode }) => {
      session.current = null;
      if (disposed) return;
      const code = exitCode ?? "unknown";
      const msg =
        job.kind === "ssh"
          ? `[Session ended, exit code ${code}. Press Enter to reconnect.]`
          : exitCode === 0
            ? "[Finished successfully.]"
            : `[Failed, exit code ${code}.]`;
      t.write(`\r\n${exitCode === 0 || job.kind === "ssh" ? dim(msg) : red(msg)}\r\n`);
      if (job.kind === "transfer" && exitCode === 0) setProgress(tab.id, 100);
      setStatus(tab.id, "exited", exitCode);
      invalidateAll(qc);
    };

    if (tab.generation > 0)
      t.write(`\r\n${dim(job.kind === "ssh" ? "[Reconnecting…]" : "[Running again…]")}\r\n`);
    const start = (cols: number, rows: number) => {
      switch (job.kind) {
        case "ssh":
          return api.terminalOpen(tab.hostId, cols, rows, onData, onExit);
        case "transfer":
          return api.transferOpen(job.spec, cols, rows, onData, onExit);
        case "script":
          return api.scriptOpen(job.run, cols, rows, onData, onExit);
      }
    };
    start(t.cols, t.rows)
      .then(({ sessionId, command }) => {
        if (disposed) {
          void api.terminalClose(sessionId).catch(() => {});
          return;
        }
        session.current = sessionId;
        for (const data of pending.current.splice(0)) void api.terminalWrite(sessionId, data);
        setStatus(tab.id, "open");
        t.write(dim(`$ ${command}`) + "\r\n");
        invalidateAll(qc);
      })
      .catch((e) => {
        if (disposed) return;
        t.write(
          `\r\n${red(`Could not start: ${errorMessage(e)}`)}\r\n${dim(job.kind === "ssh" ? "Press Enter to retry." : "Fix the problem, then use Run again.")}\r\n`,
        );
        setStatus(tab.id, "error");
      });

    return () => {
      disposed = true;
      if (session.current) void api.terminalClose(session.current).catch(() => {});
      session.current = null;
    };
  }, [tab.id, tab.hostId, tab.job, tab.generation, qc]);

  useEffect(() => {
    if (term.current) term.current.options.theme = resolvedTheme === "dark" ? DARK : LIGHT;
  }, [resolvedTheme]);

  // "Stop": end the process; the exit message and output stay in the tab.
  useEffect(() => {
    if (tab.stopSignal > 0 && session.current)
      void api.terminalClose(session.current).catch(() => {});
  }, [tab.stopSignal]);

  useEffect(() => {
    if (!active) return;
    const id = requestAnimationFrame(() => {
      fit.current?.fit();
      term.current?.focus();
    });
    return () => cancelAnimationFrame(id);
  }, [active]);

  return (
    <div
      ref={container}
      data-terminal
      className="h-full w-full bg-white px-2 pt-1 dark:bg-[#0a0a0a]"
      hidden={!active}
    />
  );
}
