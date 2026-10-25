import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { invalidateAll } from "@/features/hosts/queries";
import {
  api,
  errorMessage,
  type StepEvent,
  type TerminalExit,
  type TerminalOpened,
} from "@/lib/ipc";
import { isMac } from "@/lib/platform";
import { parseProgress, resumableRun, useTerminalStore, type TerminalTab } from "./store";
import { useTerminalAppearance } from "./use-terminal-appearance";

const dim = (s: string) => `\x1b[90m${s}\x1b[0m`;
const red = (s: string) => `\x1b[31m${s}\x1b[0m`;

export function TerminalView({ tab, active }: { tab: TerminalTab; active: boolean }) {
  const container = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  const fit = useRef<FitAddon | null>(null);
  const session = useRef<string | null>(null);
  /** Input typed (or terminal replies produced) before the session id is known. */
  const pending = useRef<string[]>([]);
  const qc = useQueryClient();
  const appearance = useTerminalAppearance();
  // Read once when the terminal is created; later changes are applied by the effect below.
  const initialAppearance = useRef(appearance);

  // One xterm instance per tab, kept across reconnects so scrollback survives.
  useEffect(() => {
    const { scheme, fontFamily, fontSize } = initialAppearance.current;
    const t = new Terminal({
      fontFamily,
      fontSize,
      cursorBlink: true,
      scrollback: 10_000,
      theme: scheme.theme,
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
      if (e.type !== "keydown") return true;
      // macOS: ⌘ shortcuts never go to the shell. ⌘C/⌘V take the native copy/paste route
      // (Edit menu → xterm's clipboard handlers), and Ctrl+C stays an interrupt.
      if (isMac) return !e.metaKey;
      if (!e.ctrlKey) return true;
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
    const { setStatus, setProgress, setRun, setStep } = useTerminalStore.getState();
    const job = tab.job;
    let disposed = false;
    // This run's own session. A superseded run (StrictMode's double mount in development, or a
    // reconnect) must never touch `session.current`, which by then belongs to the next run:
    // clearing it would drop keystrokes and ConPTY's startup cursor-position reply, leaving
    // ssh hanging on the connection line.
    let mine: string | null = null;
    pending.current = [];

    const decoder = new TextDecoder();
    const onData = new Channel<ArrayBuffer>();
    onData.onmessage = (buf) => {
      if (disposed) return;
      const bytes = new Uint8Array(buf);
      t.write(bytes);
      if (job.kind === "transfer") {
        const pct = parseProgress(decoder.decode(bytes, { stream: true }));
        if (pct !== null) setProgress(tab.id, pct);
      }
    };
    const onExit = new Channel<TerminalExit>();
    onExit.onmessage = ({ exitCode }) => {
      if (disposed) return;
      if (session.current === mine) session.current = null;
      const code = exitCode ?? "unknown";
      const msg =
        job.kind === "ssh"
          ? `[Session ended, exit code ${code}. Press Enter to reconnect.]`
          : exitCode === 0
            ? "[Finished successfully.]"
            : job.kind === "folder"
              ? exitCode === null
                ? "[Cancelled. Retry continues from the unfinished step.]"
                : "[Failed. Retry continues from the failed step.]"
              : `[Failed, exit code ${code}.]`;
      t.write(`\r\n${exitCode === 0 || job.kind === "ssh" ? dim(msg) : red(msg)}\r\n`);
      if (job.kind === "transfer" && exitCode === 0) setProgress(tab.id, 100);
      setStatus(tab.id, "exited", exitCode);
      invalidateAll(qc);
    };

    // Folder transfers: step updates can arrive before the run is known; hold them until then.
    let stepsReady = false;
    const early: StepEvent[] = [];
    const onStep = new Channel<StepEvent>();
    onStep.onmessage = (e) => {
      if (disposed) return;
      if (stepsReady) setStep(tab.id, e.index, e.status, e.detail);
      else early.push(e);
    };
    const current = useTerminalStore.getState().tabs.find((x) => x.id === tab.id);
    const resume = current ? resumableRun(current) : null;

    if (tab.generation > 0)
      t.write(
        `\r\n${dim(job.kind === "ssh" ? "[Reconnecting…]" : resume ? "[Retrying…]" : "[Running again…]")}\r\n`,
      );
    const start = (cols: number, rows: number): Promise<TerminalOpened> => {
      switch (job.kind) {
        case "ssh":
          return api.terminalOpen(tab.hostId, job.user ?? null, cols, rows, onData, onExit);
        case "transfer":
          return api.transferOpen(job.spec, cols, rows, onData, onExit);
        case "script":
          return api.scriptOpen(job.run, cols, rows, onData, onExit);
        case "folder":
          return api
            .folderOpen(job.spec, resume, job.recipeId ?? null, cols, rows, onData, onExit, onStep)
            .then((opened) => {
              if (!disposed) {
                setRun(
                  tab.id,
                  opened.runId,
                  opened.steps.map((s) => s.label),
                );
                stepsReady = true;
                for (const e of early.splice(0)) setStep(tab.id, e.index, e.status, e.detail);
              }
              return opened;
            });
      }
    };
    start(t.cols, t.rows)
      .then(({ sessionId, command }) => {
        if (disposed) {
          void api.terminalClose(sessionId).catch(() => {});
          return;
        }
        mine = sessionId;
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
      if (mine) void api.terminalClose(mine).catch(() => {});
      if (session.current === mine) session.current = null;
    };
  }, [tab.id, tab.hostId, tab.job, tab.generation, qc]);

  // Settings or the app theme changed: restyle in place (a new font size changes the grid).
  const { scheme, fontFamily, fontSize } = appearance;
  useEffect(() => {
    const t = term.current;
    if (!t) return;
    t.options.theme = scheme.theme;
    if (t.options.fontFamily !== fontFamily || t.options.fontSize !== fontSize) {
      t.options.fontFamily = fontFamily;
      t.options.fontSize = fontSize;
      if (container.current?.offsetParent) fit.current?.fit();
    }
  }, [scheme, fontFamily, fontSize]);

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

  // The fit addon sizes xterm from its parent's computed size, padding included, so the
  // padding lives on the wrapper and xterm's parent has none (otherwise it overflows, and the
  // scrollbars that appear make the size flip back and forth).
  return (
    <div
      className="absolute inset-0 overflow-hidden pt-1 pl-2"
      style={{ background: scheme.theme.background }}
      hidden={!active}
    >
      <div ref={container} data-terminal className="h-full w-full overflow-hidden" />
    </div>
  );
}
