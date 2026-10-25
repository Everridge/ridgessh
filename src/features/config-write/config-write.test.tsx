import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { AppProviders } from "@/components/app-providers";
import type { Connection } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend } from "@/test/fake-backend";
import App from "@/App";

const snapshot = (alias: string, over: Partial<Connection> = {}): Connection => ({
  alias,
  hostname: "10.0.0.5",
  user: null,
  port: null,
  identityFile: null,
  proxyJump: null,
  extraOptions: [],
  userMatches: [],
  ...over,
});

function renderApp() {
  const user = userEvent.setup();
  render(
    <AppProviders>
      <App />
    </AppProviders>,
  );
  return user;
}

beforeEach(() => {
  useUiStore.setState({
    view: "all",
    groupId: null,
    search: "",
    selectedHostId: null,
    checkedIds: [],
    hostForm: null,
    syncReviewOpen: false,
    configWrite: null,
  });
});

describe("write to config", () => {
  it("writes the previewed change only after the confirmation word is typed", async () => {
    const backend = installFakeBackend({
      hosts: [
        {
          alias: "web",
          hostname: "10.0.0.5",
          port: 2222,
          syncState: "synced",
          configDirty: true,
          configSnapshot: snapshot("web"),
        },
        { alias: "lab", syncState: "db_only" },
      ],
    });
    const user = renderApp();

    const open = await screen.findByRole("button", { name: /write config\s*1/i });
    await user.click(open);
    const dialog = await screen.findByRole("dialog", { name: /write to ssh config/i });

    // Pending edits are pre-selected, additions are opt-in.
    expect(within(dialog).getByRole("checkbox", { name: "web" })).toBeChecked();
    expect(within(dialog).getByRole("checkbox", { name: "lab" })).not.toBeChecked();
    expect(await within(dialog).findByText("Port 2222", { exact: false })).toBeInTheDocument();

    const write = within(dialog).getByRole("button", { name: /^write config$/i });
    const input = within(dialog).getByLabelText(/to write the file/i);
    expect(write).toBeDisabled();
    await user.type(input, "confirm");
    expect(write).toBeDisabled();
    await user.clear(input);
    await user.type(input, "CONFIRM");
    await waitFor(() => expect(write).toBeEnabled());
    expect(backend.calls.some((c) => c.cmd === "config_apply")).toBe(false);

    await user.click(write);
    expect(await screen.findByText(/wrote 1 change to/i)).toBeInTheDocument();
    const apply = backend.calls.find((c) => c.cmd === "config_apply")!;
    expect(apply.args).toEqual({
      changes: [{ hostId: backend.hosts[0].id, op: "update" }],
      baseHash: "base",
      resultHash: `result:${backend.hosts[0].id}`,
      confirmation: "CONFIRM",
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.queryByRole("button", { name: /write config\s*1/i })).not.toBeInTheDocument();
  });

  it("opens from the host details with only that host selected", async () => {
    installFakeBackend({
      hosts: [
        { alias: "lab", syncState: "db_only" },
        { alias: "web", configDirty: true, syncState: "synced", configSnapshot: snapshot("web") },
      ],
    });
    const user = renderApp();
    const list = await screen.findByRole("list", { name: "Hosts" });
    await user.click(within(list).getAllByText("lab")[0]);
    await user.click(await screen.findByRole("button", { name: /add to config/i }));

    const dialog = await screen.findByRole("dialog", { name: /write to ssh config/i });
    expect(within(dialog).getByRole("checkbox", { name: "lab" })).toBeChecked();
    expect(within(dialog).getByRole("checkbox", { name: "web" })).not.toBeChecked();
    await user.click(within(dialog).getByRole("checkbox", { name: "lab" }));
    expect(await within(dialog).findByText(/select at least one change/i)).toBeInTheDocument();
    expect(within(dialog).getByLabelText(/to write the file/i)).toBeDisabled();
  });

  it("shows why a conflicting host cannot be written", async () => {
    installFakeBackend({
      hosts: [
        { alias: "db", syncState: "conflict", configDirty: true, configSnapshot: snapshot("db") },
      ],
    });
    const user = renderApp();
    await user.click(await screen.findByRole("button", { name: /write config/i }));
    const dialog = await screen.findByRole("dialog", { name: /write to ssh config/i });
    expect(await within(dialog).findByText(/resolve the conflict/i)).toBeInTheDocument();
    expect(within(dialog).getByRole("checkbox", { name: "db" })).toBeDisabled();
  });
});
