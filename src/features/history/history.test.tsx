import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { AppProviders } from "@/components/app-providers";
import type { AuditEntry, Host } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend, makeHost } from "@/test/fake-backend";
import App from "@/App";

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
    search: "",
    selectedHostId: null,
    checkedIds: [],
    hostForm: null,
    syncReviewOpen: false,
    configWrite: null,
    settingsOpen: false,
    backupsOpen: false,
    historyOpen: false,
    restoreTarget: null,
  });
});

/** Snapshots in the audit log are plain JSON objects. */
const snap = (h: Host) => ({ ...h }) as unknown as Record<string, unknown>;

const entry = (over: Partial<AuditEntry>): Omit<AuditEntry, "id"> => ({
  ts: "2026-10-24T09:00:00.000Z",
  batchId: null,
  source: "user",
  entityType: "host",
  entityId: "h1",
  action: "update",
  label: "web",
  before: null,
  after: null,
  ...over,
});

describe("history", () => {
  it("groups one action's changes and undoes them together", async () => {
    const a = makeHost({ id: "a", alias: "alpha" });
    const b = makeHost({ id: "b", alias: "beta" });
    const backend = installFakeBackend({
      history: [
        entry({
          entityType: "settings",
          entityId: "app",
          label: "Settings",
          before: { confirmationWord: "CONFIRM" },
          after: { confirmationWord: "WRITE" },
        }),
        entry({
          batchId: "bulk",
          action: "delete",
          entityId: "a",
          label: "alpha",
          before: snap(a),
          after: { ...a, deletedAt: "x" },
        }),
        entry({
          batchId: "bulk",
          action: "delete",
          entityId: "b",
          label: "beta",
          before: snap(b),
          after: { ...b, deletedAt: "x" },
        }),
      ],
    });
    const user = renderApp();
    await user.click(await screen.findByRole("button", { name: /^history/i }));
    const dialog = await screen.findByRole("dialog", { name: /^history$/i });

    const list = await within(dialog).findByRole("list", { name: "Changes" });
    expect(within(list).getByText("2 changes")).toBeInTheDocument();
    expect(within(list).getAllByText("Moved to trash")).toHaveLength(2);
    expect(within(list).getByText("Confirmation word:")).toBeInTheDocument();

    await user.click(within(list).getByRole("button", { name: /undo all/i }));
    const confirm = await screen.findByRole("alertdialog");
    await user.click(within(confirm).getByRole("button", { name: /undo all/i }));
    await waitFor(() =>
      expect(backend.calls.find((c) => c.cmd === "history_revert_batch")?.args).toEqual({
        batchId: "bulk",
      }),
    );

    await user.type(within(dialog).getByLabelText(/search history/i), "beta");
    await waitFor(() =>
      expect(
        backend.calls.some(
          (c) =>
            c.cmd === "history_list" && (c.args.filter as { search?: string }).search === "beta",
        ),
      ).toBe(true),
    );
  });

  it("undoes or restores a version from the host's own history", async () => {
    const web = makeHost({ id: "id-1", alias: "web" });
    const backend = installFakeBackend({
      hosts: [{ alias: "web", notes: "new" }],
      history: [
        entry({
          entityId: "id-1",
          before: { ...web, notes: "old" },
          after: { ...web, notes: "new" },
        }),
      ],
    });
    const user = renderApp();
    const list = await screen.findByRole("list", { name: "Hosts" });
    await user.click(within(list).getAllByText("web")[0]);

    await user.click(await screen.findByRole("tab", { name: /history/i }));
    expect(await screen.findByText("Notes:")).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: /^undo$/i }));
    await user.click(screen.getByRole("button", { name: /restore this version/i }));
    const reverts = backend.calls
      .filter((c) => c.cmd === "history_revert")
      .map((c) => c.args.target);
    expect(reverts).toEqual(["before", "after"]);
  });

  it("empties the trash but keeps hosts that are still in the config", async () => {
    const backend = installFakeBackend({
      hosts: [
        { alias: "gone", deletedAt: "2026-10-01T00:00:00.000Z" },
        {
          alias: "still-there",
          deletedAt: "2026-10-01T00:00:00.000Z",
          syncState: "synced",
          configSnapshot: {
            alias: "still-there",
            hostname: null,
            user: null,
            port: null,
            identityFile: null,
            proxyJump: null,
            extraOptions: [],
          },
        },
      ],
    });
    useUiStore.setState({ view: "trash" });
    const user = renderApp();
    await user.click(await screen.findByRole("button", { name: /empty trash/i }));
    const confirm = await screen.findByRole("alertdialog");
    expect(within(confirm).getByText(/permanently delete 1 host\?/i)).toBeInTheDocument();
    expect(within(confirm).getByText(/1 host is still in ~\/.ssh\/config/i)).toBeInTheDocument();
    await user.click(within(confirm).getByRole("button", { name: /delete permanently/i }));
    await waitFor(() =>
      expect(backend.calls.some((c) => c.cmd === "hosts_empty_trash")).toBe(true),
    );
  });
});
