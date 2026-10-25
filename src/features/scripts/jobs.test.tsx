import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProviders } from "@/components/app-providers";
import { useTerminalStore } from "@/features/terminal/store";
import type { Script } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend } from "@/test/fake-backend";
import App from "@/App";

// xterm needs a real canvas; the tab contents are not under test here.
vi.mock("@/features/terminal/terminal-view", () => ({ TerminalView: () => null }));

function renderApp() {
  const user = userEvent.setup();
  render(
    <AppProviders>
      <App />
    </AppProviders>,
  );
  return user;
}

const script = (over: Partial<Script> = {}): Script => ({
  id: "s1",
  name: "Disk usage",
  description: "df",
  content: "df -h\n",
  interpreter: "bash",
  defaultArgs: "--total",
  runMode: "stream",
  createdAt: "2026-10-24T10:00:00.000Z",
  updatedAt: "2026-10-24T10:00:00.000Z",
  ...over,
});

beforeEach(() => {
  useUiStore.setState({
    view: "all",
    selectedHostId: null,
    checkedIds: [],
    scriptsOpen: false,
    scriptRun: null,
    transfer: null,
  });
  useTerminalStore.setState({ tabs: [], activeId: null });
});

describe("scripts and transfers", () => {
  it("runs a script on each selected host in its own tab", async () => {
    const backend = installFakeBackend({
      hosts: [{ alias: "web" }, { alias: "db" }, { alias: "other" }],
      scripts: [script()],
    });
    const user = renderApp();
    const list = await screen.findByRole("list", { name: "Hosts" });
    const [db, web] = [backend.hosts[1], backend.hosts[0]];
    useUiStore.getState().setChecked([web.id, db.id]);
    await user.click(await screen.findByRole("button", { name: /^run script$/i }));
    expect(list).toBeInTheDocument();

    const dialog = await screen.findByRole("dialog", { name: /run script/i });
    const targets = within(dialog).getByRole("list", { name: "Target hosts" });
    expect(within(targets).getByRole("checkbox", { name: "web" })).toBeChecked();
    expect(within(targets).getByRole("checkbox", { name: "other" })).not.toBeChecked();
    const run = within(dialog).getByRole("button", { name: /run on 2 hosts/i });
    expect(run).toBeDisabled(); // no script chosen yet

    await user.click(within(dialog).getByRole("combobox", { name: "Script" }));
    await user.click(await screen.findByRole("option", { name: "Disk usage" }));
    const args = within(dialog).getByLabelText(/arguments/i);
    await waitFor(() => expect(args).toHaveValue("--total"));
    await user.clear(args);
    await user.type(args, `--path "/var/log"`);
    await user.click(within(dialog).getByRole("button", { name: /run on 2 hosts/i }));

    const tabs = useTerminalStore.getState().tabs;
    expect(tabs.map((t) => t.title).sort()).toEqual(["▶ Disk usage · db", "▶ Disk usage · web"]);
    expect(tabs[0].job).toEqual({
      kind: "script",
      run: {
        hostId: tabs[0].hostId,
        scriptId: "s1",
        mode: "stream",
        args: `--path "/var/log"`,
        user: null,
      },
    });
    expect(await screen.findByRole("tab", { name: /disk usage · web/i })).toBeInTheDocument();
  });

  it("saves a new script in the library", async () => {
    const backend = installFakeBackend({});
    const user = renderApp();
    await user.click(await screen.findByRole("button", { name: /^scripts/i }));
    const dialog = await screen.findByRole("dialog", { name: /^scripts$/i });
    await user.click(within(dialog).getByRole("button", { name: /new script/i }));
    await user.type(within(dialog).getByLabelText(/^name$/i), "Uptime");
    const body = within(dialog).getByLabelText(/^script$/i);
    await user.clear(body);
    await user.type(body, "uptime");
    await user.click(within(dialog).getByRole("button", { name: /^save$/i }));

    expect(await within(dialog).findByRole("button", { name: /uptime/i })).toBeInTheDocument();
    expect(backend.calls.find((c) => c.cmd === "script_create")?.args).toEqual({
      input: {
        name: "Uptime",
        description: "",
        content: "uptime",
        interpreter: "bash",
        defaultArgs: "",
        runMode: "stream",
      },
    });
  });

  it("uploads picked files to the last used remote folder", async () => {
    const backend = installFakeBackend({
      hosts: [{ alias: "web" }],
      lastRemote: "/srv/app",
      dialogResult: ["C:\\work\\build.zip"],
    });
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    useUiStore.getState().openTransfer(backend.hosts[0].id, "upload");
    const dialog = await screen.findByRole("dialog", { name: /copy files/i });
    expect(await within(dialog).findByDisplayValue("/srv/app")).toBeInTheDocument();
    const upload = within(dialog).getByRole("button", { name: /^upload$/i });
    expect(upload).toBeDisabled();

    await user.click(within(dialog).getByRole("button", { name: /add files/i }));
    expect(await within(dialog).findByText("C:\\work\\build.zip")).toBeInTheDocument();
    await user.click(upload);

    const [tab] = useTerminalStore.getState().tabs;
    expect(tab.title).toBe("↑ build.zip → web");
    expect(tab.job).toEqual({
      kind: "transfer",
      spec: {
        hostId: backend.hosts[0].id,
        direction: "upload",
        local: ["C:\\work\\build.zip"],
        remote: "/srv/app",
        recursive: false,
        user: null,
      },
    });
  });
});
