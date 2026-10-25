import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProviders } from "@/components/app-providers";
import { useTerminalStore } from "@/features/terminal/store";
import type { UserVariant } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend, type FakeSeed } from "@/test/fake-backend";
import App from "@/App";

vi.mock("@/features/terminal/terminal-view", () => ({ TerminalView: () => null }));

const variant = (user: string, key: string, line: number): UserVariant => ({
  alias: "devbox",
  user,
  identityFile: `c:\\Users\\me\\.ssh\\${key}`,
  options: [{ key: "IdentityFile", value: `c:\\Users\\me\\.ssh\\${key}` }],
  source: "C:\\Users\\me\\.ssh\\config",
  line,
});

const seed = (): FakeSeed => ({
  hosts: [
    {
      alias: "devbox",
      hostname: "203.0.113.10",
      configSnapshot: {
        alias: "devbox",
        hostname: "203.0.113.10",
        user: null,
        port: null,
        identityFile: null,
        proxyJump: null,
        extraOptions: [],
      },
    },
    { alias: "web", hostname: "10.0.0.1" },
  ],
  variants: [variant("tom", "devbox", 235), variant("deploy", "devbox_deploy", 232)],
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
  useTerminalStore.setState({ tabs: [], activeId: null });
  useUiStore.setState({
    view: "all",
    search: "",
    selectedHostId: null,
    checkedIds: [],
    transfer: null,
  });
});

describe("per-user Match blocks", () => {
  it("are listed under their host and connect as user@alias", async () => {
    const backend = installFakeBackend(seed());
    const user = renderApp();
    const users = await screen.findByRole("list", { name: "Users of devbox" });
    const rows = within(users).getAllByRole("button");
    expect(rows.map((r) => r.textContent)).toEqual([
      "deploy@devboxdevbox_deploy",
      "tom@devboxdevbox",
    ]);
    expect(screen.queryByRole("list", { name: "Users of web" })).not.toBeInTheDocument();

    await user.dblClick(rows[1]);
    const tab = useTerminalStore.getState().tabs[0];
    expect(tab).toMatchObject({
      hostId: backend.hosts[0].id,
      title: "tom@devbox",
      job: { kind: "ssh", user: "tom" },
    });

    // Collapsing hides them.
    await user.click(screen.getByRole("button", { name: "Hide users of devbox" }));
    expect(screen.queryByRole("list", { name: "Users of devbox" })).not.toBeInTheDocument();
  });

  it("are found by searching for the user", async () => {
    installFakeBackend(seed());
    renderApp();
    await screen.findByRole("list", { name: "Users of devbox" });
    useUiStore.setState({ search: "tom" });
    await waitFor(() =>
      expect(
        within(screen.getByRole("list", { name: "Hosts" })).getAllByRole("listitem"),
      ).not.toHaveLength(0),
    );
    expect(screen.queryByText("web")).not.toBeInTheDocument();
    expect(screen.getByText("tom@devbox")).toBeInTheDocument();
  });

  it("show in the details panel and open transfers as that user", async () => {
    const backend = installFakeBackend(seed());
    const user = renderApp();
    await screen.findByRole("list", { name: "Users of devbox" });
    useUiStore.getState().selectHost(backend.hosts[0].id);
    const details = await screen.findByRole("list", { name: "Users" });
    await user.click(within(details).getByRole("button", { name: "Connect as deploy" }));
    expect(useTerminalStore.getState().tabs[0].title).toBe("deploy@devbox");

    useUiStore.getState().openTransfer(backend.hosts[0].id, "download", "tom");
    const dialog = await screen.findByRole("dialog", { name: "Copy files — tom@devbox" });
    expect(within(dialog).getByRole("combobox", { name: "Connect as" })).toHaveTextContent("tom");
  });
});

describe("same-server hosts", () => {
  const servers = (): FakeSeed => ({
    hosts: [
      { alias: "acme_prod_deploy", hostname: "203.0.113.20", user: "deploy" },
      { alias: "acme_prod_root", hostname: "203.0.113.20", user: "root" },
      { alias: "web", hostname: "10.0.0.1" },
    ],
  });

  it("nest under one server entry that selects them together", async () => {
    installFakeBackend(servers());
    const user = renderApp();
    const nested = await screen.findByRole("list", { name: "Hosts on acme_prod" });
    expect(
      within(nested)
        .getAllByText(/^acme_prod_/)
        .map((e) => e.textContent),
    ).toEqual(["acme_prod_deploy", "acme_prod_root"]);
    expect(screen.getByText("203.0.113.20 · deploy · root")).toBeInTheDocument();

    await user.click(screen.getByRole("checkbox", { name: "Select all hosts on acme_prod" }));
    expect(useUiStore.getState().checkedIds).toHaveLength(2);

    await user.click(screen.getByRole("button", { name: "Server acme_prod, 2 hosts" }));
    expect(screen.queryByRole("list", { name: "Hosts on acme_prod" })).not.toBeInTheDocument();
  });

  it("can be shown flat", async () => {
    installFakeBackend(servers());
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts on acme_prod" });
    await user.click(screen.getByRole("button", { name: "Group hosts by server" }));
    expect(screen.queryByRole("list", { name: "Hosts on acme_prod" })).not.toBeInTheDocument();
    expect(screen.getByText("acme_prod_root")).toBeInTheDocument();
    useUiStore.getState().setGroupServers(true);
  });
});
