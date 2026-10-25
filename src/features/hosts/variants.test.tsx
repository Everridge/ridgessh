import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProviders } from "@/components/app-providers";
import { useTerminalStore } from "@/features/terminal/store";
import type { UserMatch } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend, type FakeSeed } from "@/test/fake-backend";
import App from "@/App";
import { toFormValues, toHostInput } from "./host-form";

vi.mock("@/features/terminal/terminal-view", () => ({ TerminalView: () => null }));

const userMatch = (user: string, key: string): UserMatch => ({
  user,
  identityFile: `c:\\Users\\me\\.ssh\\${key}`,
  extraOptions: [],
});

const users = [userMatch("tom", "devbox"), userMatch("deploy", "devbox_deploy")];

const seed = (): FakeSeed => ({
  hosts: [
    {
      alias: "devbox",
      hostname: "203.0.113.10",
      syncState: "synced",
      userMatches: users,
      configSnapshot: {
        alias: "devbox",
        hostname: "203.0.113.10",
        user: null,
        port: null,
        identityFile: null,
        proxyJump: null,
        extraOptions: [],
        userMatches: users,
      },
    },
    { alias: "web", hostname: "10.0.0.1" },
  ],
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

describe("editing users", () => {
  it("changes, removes and adds users of a host", async () => {
    const backend = installFakeBackend(seed());
    const user = renderApp();
    await screen.findByRole("list", { name: "Users of devbox" });
    useUiStore.getState().openHostForm(backend.hosts[0].id, "users");

    const dialog = await screen.findByRole("dialog", { name: "Edit devbox" });
    const list = within(dialog).getByRole("list", { name: "Users" });
    expect(within(list).getByLabelText("User 1")).toHaveValue("tom");
    expect(within(dialog).getByText(/Match originalhost devbox user/)).toBeInTheDocument();

    await user.click(within(list).getByRole("button", { name: "Remove user 1" }));
    const key = within(list).getByLabelText("Identity file of user 1");
    await user.clear(key);
    await user.type(key, "~/.ssh/deploy2");
    await user.click(within(dialog).getByRole("button", { name: "Add user" }));
    await user.type(within(list).getByLabelText("User 2"), "ops");
    await user.type(within(list).getByLabelText("Identity file of user 2"), "~/.ssh/ops");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "Edit devbox" })).not.toBeInTheDocument(),
    );
    const saved = backend.calls.find((c) => c.cmd === "host_update")!;
    expect((saved.args.input as { userMatches: UserMatch[] }).userMatches).toEqual([
      { user: "deploy", identityFile: "~/.ssh/deploy2", extraOptions: [] },
      { user: "ops", identityFile: "~/.ssh/ops", extraOptions: [] },
    ]);
    const rows = within(await screen.findByRole("list", { name: "Users of devbox" }));
    await waitFor(() => expect(rows.getByText("ops@devbox")).toBeInTheDocument());
    expect(rows.queryByText("tom@devbox")).not.toBeInTheDocument();
  });

  it("adds the first user to a regular host and checks the names", async () => {
    const backend = installFakeBackend(seed());
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    useUiStore.getState().openHostForm(backend.hosts[1].id, "users");

    const dialog = await screen.findByRole("dialog", { name: "Edit web" });
    expect(within(dialog).getByText(/connects only as its own user/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Add user" }));
    await user.click(within(dialog).getByRole("button", { name: "Add user" }));
    await user.type(within(dialog).getByLabelText("User 1"), "app");
    await user.type(within(dialog).getByLabelText("User 2"), "app");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByText("app is listed twice")).toBeInTheDocument();
    expect(backend.calls.some((c) => c.cmd === "host_update")).toBe(false);

    await user.click(within(dialog).getByRole("button", { name: "Remove user 2" }));
    await user.type(within(dialog).getByLabelText("Identity file of user 1"), "~/.ssh/web_app");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("list", { name: "Users of web" })).toHaveTextContent("app@web");
  });

  it("keep their other options through single-field edits", () => {
    const host = installFakeBackend(seed()).hosts[0];
    const kept = { ...users[0], extraOptions: [{ key: "ForwardAgent", value: "yes" }] };
    const input = toHostInput(toFormValues({ ...host, userMatches: [kept] }));
    expect(input.userMatches).toEqual([kept]);
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
