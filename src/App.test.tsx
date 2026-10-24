import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { AppProviders } from "@/components/app-providers";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend } from "@/test/fake-backend";
import App from "./App";

function renderApp() {
  const user = userEvent.setup();
  render(
    <AppProviders>
      <App />
    </AppProviders>,
  );
  return user;
}

const hostList = () => screen.getByRole("list", { name: "Hosts" });
const findHostList = () => screen.findByRole("list", { name: "Hosts" });

beforeEach(() => {
  useUiStore.setState({
    view: "all",
    groupId: null,
    search: "",
    selectedHostId: null,
    checkedIds: [],
    hostForm: null,
  });
});

describe("hosts", () => {
  it("creates a host through the form and shows it in list and details", async () => {
    const backend = installFakeBackend();
    const user = renderApp();

    await user.click(await screen.findByRole("button", { name: /new host/i }));
    const dialog = await screen.findByRole("dialog");
    await user.type(within(dialog).getByLabelText(/alias/i), "prod-db");
    await user.type(within(dialog).getByLabelText(/hostname/i), "10.0.0.5");
    await user.type(within(dialog).getByLabelText(/^port$/i), "2222");
    await user.type(within(dialog).getByLabelText(/^user$/i), "deploy");
    await user.click(within(dialog).getByRole("button", { name: /create host/i }));

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(within(hostList()).getByText("prod-db")).toBeInTheDocument();
    expect(screen.getAllByText("deploy@10.0.0.5:2222").length).toBeGreaterThan(0);

    const create = backend.calls.find((c) => c.cmd === "host_create");
    expect(create?.args.input).toMatchObject({
      alias: "prod-db",
      hostname: "10.0.0.5",
      port: 2222,
      user: "deploy",
      identityFile: null,
    });
  });

  it("blocks invalid input before calling the backend", async () => {
    const backend = installFakeBackend();
    const user = renderApp();

    await user.click(await screen.findByRole("button", { name: /new host/i }));
    const dialog = await screen.findByRole("dialog");
    await user.type(within(dialog).getByLabelText(/alias/i), "bad alias");
    await user.type(within(dialog).getByLabelText(/hostname/i), "-oProxyCommand=calc");
    await user.click(within(dialog).getByRole("button", { name: /create host/i }));

    expect(await within(dialog).findByText(/alias cannot contain spaces/i)).toBeInTheDocument();
    expect(within(dialog).getByText(/hostname cannot start with "-"/i)).toBeInTheDocument();
    expect(backend.calls.some((c) => c.cmd === "host_create")).toBe(false);
  });

  it("shows backend conflicts as an error and keeps the form open", async () => {
    installFakeBackend({ hosts: [{ alias: "web" }] });
    const user = renderApp();

    await user.click(await screen.findByRole("button", { name: /new host/i }));
    const dialog = await screen.findByRole("dialog");
    await user.type(within(dialog).getByLabelText(/alias/i), "web");
    await user.click(within(dialog).getByRole("button", { name: /create host/i }));

    expect(await screen.findByText(/already exists/i)).toBeInTheDocument();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("moves a host to trash and restores it", async () => {
    installFakeBackend({ hosts: [{ alias: "old-box", hostname: "1.2.3.4" }] });
    const user = renderApp();

    await user.click(within(await findHostList()).getByText("old-box"));
    await user.click(screen.getByRole("button", { name: /^delete$/i }));
    await waitFor(() =>
      expect(screen.queryByRole("list", { name: "Hosts" })).not.toBeInTheDocument(),
    );
    expect(await screen.findByText(/moved to trash/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^trash/i }));
    await user.click(within(await findHostList()).getByText("old-box"));
    await user.click(screen.getByRole("button", { name: /restore/i }));

    await user.click(screen.getByRole("button", { name: /all hosts/i }));
    expect(within(await findHostList()).getByText("old-box")).toBeInTheDocument();
  });

  it("records edits in the host history", async () => {
    installFakeBackend({ hosts: [{ alias: "api", port: 22 }] });
    const user = renderApp();

    await user.dblClick(within(await findHostList()).getByText("api", { selector: "span" }));
    const dialog = await screen.findByRole("dialog");
    const port = within(dialog).getByLabelText(/^port$/i);
    await user.clear(port);
    await user.type(port, "2200");
    await user.click(within(dialog).getByRole("button", { name: /^save$/i }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());

    await user.click(within(hostList()).getByText("api", { selector: "span" }));
    const tab = screen.getByRole("tab", { name: /history/i });
    await user.click(tab);
    await waitFor(() => expect(tab).toHaveAttribute("aria-selected", "true"));
    const panel = screen.getByRole("tabpanel");
    expect(await within(panel).findByText(/→ 2200/)).toBeInTheDocument();
    expect(within(panel).getByText("Updated")).toBeInTheDocument();
    expect(within(panel).getByText("Port:")).toBeInTheDocument();
  });
});

describe("groups", () => {
  it("creates a group and filters hosts by it", async () => {
    installFakeBackend({ hosts: [{ alias: "in-group" }, { alias: "outside" }] });
    const user = renderApp();

    await user.click(await screen.findByRole("button", { name: /new group/i }));
    const dialog = await screen.findByRole("dialog");
    await user.type(within(dialog).getByLabelText(/name/i), "Production");
    await user.click(within(dialog).getByRole("button", { name: /^create$/i }));

    const groupButton = await screen.findByRole("button", { name: /production/i });
    await user.click(groupButton);
    expect(await screen.findByText(/this group has no hosts/i)).toBeInTheDocument();
  });
});
