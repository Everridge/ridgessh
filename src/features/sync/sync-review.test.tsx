import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { AppProviders } from "@/components/app-providers";
import type { Connection } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend } from "@/test/fake-backend";
import App from "@/App";

const snapshot = (over: Partial<Connection> = {}): Connection => ({
  alias: "db",
  hostname: "10.0.0.5",
  user: "admin",
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
  });
});

describe("config sync", () => {
  it("scans on startup and offers a review when something changed", async () => {
    const backend = installFakeBackend({
      hosts: [
        { alias: "web", syncState: "config_only_new", configSnapshot: snapshot({ alias: "web" }) },
      ],
      report: { added: ["web"], removed: ["old"] },
    });
    const user = renderApp();

    expect(await screen.findByText(/ssh config scanned: 1 new, 1 removed/i)).toBeInTheDocument();
    expect(backend.calls.filter((c) => c.cmd === "sync_run")).toHaveLength(1);

    await user.click(screen.getByRole("button", { name: /^review$/i }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("web", { selector: ".font-medium" })).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: /mark all reviewed/i }));
    expect(await within(dialog).findByText(/no new hosts/i)).toBeInTheDocument();
  });

  it("stays quiet when the scan found nothing", async () => {
    installFakeBackend({ hosts: [{ alias: "web", syncState: "synced" }] });
    renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    await waitFor(() => expect(screen.queryByText(/ssh config scanned/i)).not.toBeInTheDocument());
  });

  it("shows conflicts side by side and resolves with the config version", async () => {
    const backend = installFakeBackend({
      hosts: [
        {
          id: "h1",
          alias: "db",
          hostname: "10.0.0.5",
          user: "deploy",
          port: 2222,
          syncState: "conflict",
          configDirty: true,
          configSnapshot: snapshot(),
        },
      ],
      report: { conflicts: ["db"] },
    });
    const user = renderApp();
    await user.click(await screen.findByRole("button", { name: /config review/i }));

    const dialog = await screen.findByRole("dialog");
    // Opens on the conflicts tab because it has items.
    const table = within(dialog).getByRole("table");
    const userRow = within(table).getByText("User").closest("tr")!;
    expect(within(userRow).getByText("deploy")).toBeInTheDocument();
    expect(within(userRow).getByText("admin")).toBeInTheDocument();

    await user.click(within(dialog).getByRole("button", { name: /use config version/i }));
    await waitFor(() =>
      expect(backend.calls.find((c) => c.cmd === "sync_resolve_conflict")?.args).toEqual({
        id: "h1",
        keep: "config",
      }),
    );
    expect(await within(dialog).findByText(/no conflicts/i)).toBeInTheDocument();
  });

  it("merges a suggested rename", async () => {
    const backend = installFakeBackend({
      hosts: [
        {
          id: "old",
          alias: "box",
          hostname: "1.2.3.4",
          syncState: "removed_from_config",
          notes: "keep me",
        },
        { id: "new", alias: "box2", hostname: "1.2.3.4", syncState: "config_only_new" },
      ],
      renames: [{ fromId: "old", toId: "new" }],
    });
    const user = renderApp();
    await user.click(await screen.findByRole("button", { name: /config review/i }));
    const dialog = await screen.findByRole("dialog");

    await user.click(within(dialog).getByRole("button", { name: /^merge$/i }));
    await waitFor(() =>
      expect(backend.calls.find((c) => c.cmd === "sync_accept_rename")?.args).toEqual({
        fromId: "old",
        toId: "new",
      }),
    );
    expect(await within(dialog).findByText(/no renamed hosts/i)).toBeInTheDocument();
  });

  it("can keep hosts that were removed from the config", async () => {
    const backend = installFakeBackend({
      hosts: [{ id: "r1", alias: "gone", syncState: "removed_from_config" }],
    });
    const user = renderApp();
    await user.click(await screen.findByRole("button", { name: /config review/i }));
    const dialog = await screen.findByRole("dialog");

    await user.click(within(dialog).getByRole("button", { name: /keep in ridgessh/i }));
    await waitFor(() =>
      expect(backend.calls.find((c) => c.cmd === "sync_keep_db_only")?.args).toEqual({
        ids: ["r1"],
      }),
    );
  });
});
