import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProviders } from "@/components/app-providers";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend } from "@/test/fake-backend";
import App from "@/App";
import { useTerminalStore } from "./store";

// xterm needs a real layout engine; the view's wiring is exercised in the running app.
vi.mock("./terminal-view", () => ({
  TerminalView: ({ tab, active }: { tab: { title: string }; active: boolean }) => (
    <div data-testid="terminal-view" data-active={active}>
      {tab.title}
    </div>
  ),
}));

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
    quickConnectOpen: false,
  });
  useTerminalStore.setState({ tabs: [], activeId: null });
});

describe("connecting", () => {
  it("double-clicking a host opens a terminal tab", async () => {
    installFakeBackend({ hosts: [{ id: "h1", alias: "web", hostname: "10.0.0.1" }] });
    const user = renderApp();
    const list = await screen.findByRole("list", { name: "Hosts" });
    await user.dblClick(within(list).getByText("web", { selector: "span" }));

    const tablist = screen.getByRole("tablist", { name: /terminal sessions/i });
    expect(within(tablist).getByRole("tab", { name: /web/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(useTerminalStore.getState().tabs[0]).toMatchObject({ hostId: "h1", title: "web" });
  });

  it("quick connect searches by hostname and tag", async () => {
    installFakeBackend({
      hosts: [
        { id: "h1", alias: "web", hostname: "10.0.0.1" },
        {
          id: "h2",
          alias: "db",
          hostname: "10.0.0.2",
          tags: [{ id: "t", name: "postgres", color: null }],
        },
      ],
    });
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts" });

    await user.keyboard("{Control>}k{/Control}");
    const input = await screen.findByPlaceholderText(/connect to/i);
    await user.type(input, "postgres");
    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(1);
    expect(options[0]).toHaveTextContent("db");
    await user.keyboard("{Enter}");

    await waitFor(() => expect(useTerminalStore.getState().tabs).toHaveLength(1));
    expect(useTerminalStore.getState().tabs[0].hostId).toBe("h2");
    expect(screen.queryByPlaceholderText(/connect to/i)).not.toBeInTheDocument();
  });

  it("opens an external terminal from the details panel", async () => {
    const backend = installFakeBackend({ hosts: [{ id: "h1", alias: "web" }] });
    const user = renderApp();
    const list = await screen.findByRole("list", { name: "Hosts" });
    await user.click(within(list).getByText("web", { selector: "span" }));
    await user.click(screen.getByRole("button", { name: /open in external terminal/i }));
    await waitFor(() =>
      expect(backend.calls.find((c) => c.cmd === "terminal_open_external")?.args).toEqual({
        hostId: "h1",
      }),
    );
  });
});
