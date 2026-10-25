import { StrictMode } from "react";
import { render, waitFor } from "@testing-library/react";
import type { Channel } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProviders } from "@/components/app-providers";
import { api, type FolderSpec, type StepEvent, type TerminalExit } from "@/lib/ipc";
import { installFakeBackend } from "@/test/fake-backend";
import { resumableRun, useTerminalStore } from "./store";
import { TerminalView } from "./terminal-view";

// A stand-in for xterm: keeps what the view registers so the test can "type".
const xterm = vi.hoisted(() => ({
  onData: null as ((d: string) => void) | null,
  options: [] as Record<string, unknown>[],
}));
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    options: Record<string, unknown>;
    constructor(options: Record<string, unknown>) {
      this.options = { ...options };
      xterm.options.push(this.options);
    }
    open() {}
    loadAddon() {}
    onData(cb: (d: string) => void) {
      xterm.onData = cb;
    }
    onBinary() {}
    onResize() {}
    attachCustomKeyEventHandler() {}
    write() {}
    focus() {}
    dispose() {}
    hasSelection() {
      return false;
    }
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {}
  },
}));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: class {} }));

beforeEach(() => {
  xterm.onData = null;
  xterm.options = [];
  useTerminalStore.setState({ tabs: [], activeId: null });
});

describe("terminal view", () => {
  it("keeps input flowing to the live session when a superseded one exits", async () => {
    installFakeBackend({});
    const exits: Channel<TerminalExit>[] = [];
    let n = 0;
    vi.spyOn(api, "terminalOpen").mockImplementation(async (...args) => {
      exits.push(args[5]);
      return { sessionId: `s${++n}`, command: "ssh web" };
    });
    const write = vi.spyOn(api, "terminalWrite").mockResolvedValue(undefined);
    const close = vi.spyOn(api, "terminalClose").mockResolvedValue(undefined);

    const id = useTerminalStore.getState().open({ id: "h1", alias: "web" });
    const tab = useTerminalStore.getState().tabs.find((t) => t.id === id)!;
    // StrictMode mounts twice in development: the first session is started, then dropped.
    render(
      <StrictMode>
        <AppProviders>
          <TerminalView tab={tab} active />
        </AppProviders>
      </StrictMode>,
    );
    await waitFor(() => expect(exits).toHaveLength(2));
    await waitFor(() => expect(close).toHaveBeenCalledWith("s1"));

    // The dropped session's exit arrives late; it must not detach the live one.
    exits[0].onmessage({ exitCode: null });
    xterm.onData!("\x1b[1;1R"); // ConPTY's cursor-position reply
    xterm.onData!("ls\r");
    expect(write.mock.calls).toEqual([
      ["s2", "\x1b[1;1R"],
      ["s2", "ls\r"],
    ]);
    expect(close).not.toHaveBeenCalledWith("s2");
  });

  it("uses the colour scheme and font from settings", async () => {
    installFakeBackend({
      settings: {
        terminalTheme: "dracula",
        terminalFontFamily: "JetBrains Mono",
        terminalFontSize: 15,
      },
    });
    vi.spyOn(api, "terminalOpen").mockResolvedValue({ sessionId: "s1", command: "ssh web" });
    const id = useTerminalStore.getState().open({ id: "h1", alias: "web" });
    const tab = useTerminalStore.getState().tabs.find((t) => t.id === id)!;
    const { container } = render(
      <AppProviders>
        <TerminalView tab={tab} active />
      </AppProviders>,
    );
    await waitFor(() =>
      expect(xterm.options.at(-1)).toMatchObject({
        theme: { background: "#282a36", red: "#ff5555" },
        fontSize: 15,
        fontFamily: expect.stringMatching(/^"JetBrains Mono", /),
      }),
    );
    // The padding around the terminal matches its background.
    expect(
      (container.querySelector("[data-terminal]")!.parentElement as HTMLElement).style.background,
    ).toBe("rgb(40, 42, 54)");
  });

  it("tracks a folder transfer's steps and retries from the failed one", async () => {
    installFakeBackend({});
    const calls: {
      resume: string | null;
      exit: Channel<TerminalExit>;
      step: Channel<StepEvent>;
    }[] = [];
    vi.spyOn(api, "folderOpen").mockImplementation(
      async (_spec, resume, _recipe, _c, _r, _data, exit, step) => {
        calls.push({ resume, exit, step });
        // The backend starts at once: a step update can beat the reply.
        step.onmessage({ index: 0, status: "running", detail: null });
        return {
          sessionId: `route-${calls.length}`,
          command: "zip /srv/app on web → C:\\dl",
          runId: "run1",
          steps: [
            { kind: "check", label: "Check" },
            { kind: "download", label: "Download" },
          ],
        };
      },
    );
    vi.spyOn(api, "terminalClose").mockResolvedValue(undefined);
    const spec: FolderSpec = {
      source: { kind: "remote", hostId: "h1", path: "/srv/app" },
      destination: { kind: "local", path: "C:\\dl" },
      unpack: false,
      keepZip: true,
      existing: "backup",
    };
    useTerminalStore.getState().openJob({ id: "h1", alias: "web" }, { kind: "folder", spec }, "x");
    function Live() {
      const tab = useTerminalStore((s) => s.tabs[0]);
      return <TerminalView tab={tab} active />;
    }
    render(
      <AppProviders>
        <Live />
      </AppProviders>,
    );
    const tab = () => useTerminalStore.getState().tabs[0];
    await waitFor(() =>
      expect(tab().run?.steps.map((s) => s.status)).toEqual(["running", "pending"]),
    );
    expect(calls[0].resume).toBeNull();

    calls[0].step.onmessage({ index: 0, status: "done", detail: null });
    calls[0].step.onmessage({ index: 1, status: "failed", detail: "scp failed" });
    calls[0].exit.onmessage({ exitCode: 1 });
    await waitFor(() => expect(tab().status).toBe("exited"));
    expect(resumableRun(tab())).toBe("run1");

    useTerminalStore.getState().reconnect(tab().id);
    await waitFor(() => expect(calls).toHaveLength(2));
    expect(calls[1].resume).toBe("run1");
  });
});
