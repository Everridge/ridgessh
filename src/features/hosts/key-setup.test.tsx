import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProviders } from "@/components/app-providers";
import { useTerminalStore } from "@/features/terminal/store";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend } from "@/test/fake-backend";
import App from "@/App";
import { newKeyPath } from "./host-form";

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

beforeEach(() => {
  useTerminalStore.setState({ tabs: [], activeId: null });
  useUiStore.setState({
    view: "all",
    search: "",
    selectedHostId: null,
    checkedIds: [],
    hostForm: null,
  });
});

const box = (name: RegExp) => screen.getByRole("checkbox", { name });

describe("newKeyPath", () => {
  it("names the key after the alias", () => {
    expect(newKeyPath("prod-db")).toBe("~/.ssh/prod-db_ed25519");
    expect(newKeyPath(" web.example.com ")).toBe("~/.ssh/web.example.com_ed25519");
    expect(newKeyPath("a/b\\c")).toBe("~/.ssh/a_b_c_ed25519");
    expect(newKeyPath("..x")).toBe("~/.ssh/x_ed25519");
    expect(newKeyPath("")).toBe("~/.ssh/host_ed25519");
  });
});

describe("key setup for a new host", () => {
  it("generates a key, creates the host and opens the install tab", async () => {
    const backend = installFakeBackend();
    const user = renderApp();
    act(() => useUiStore.getState().openHostForm("new"));

    await user.type(await screen.findByLabelText("Alias (Host)"), "prod-db");
    await user.type(screen.getByLabelText("Hostname"), "10.0.0.5");
    await user.type(screen.getByLabelText("Identity file"), "~/.ssh/typed");
    expect(box(/install the public key/i)).toBeEnabled();

    await user.click(box(/generate a new key pair/i));
    const identity = screen.getByLabelText("Identity file");
    expect(identity).toHaveValue("~/.ssh/prod-db_ed25519");
    expect(identity).toHaveAttribute("readonly");

    // The path follows the alias.
    await user.type(screen.getByLabelText("Alias (Host)"), "2");
    expect(identity).toHaveValue("~/.ssh/prod-db2_ed25519");

    // Unchecking restores what was typed.
    await user.click(box(/generate a new key pair/i));
    expect(identity).toHaveValue("~/.ssh/typed");
    await user.click(box(/generate a new key pair/i));

    await user.click(box(/install the public key/i));
    await user.click(box(/forget the server's saved host key/i));
    await user.click(screen.getByRole("button", { name: "Create host" }));

    await waitFor(() => expect(useUiStore.getState().hostForm).toBeNull());
    const call = backend.calls.find((c) => c.cmd === "host_create_with_key");
    expect(call?.args.input).toMatchObject({
      alias: "prod-db2",
      identityFile: "~/.ssh/prod-db2_ed25519",
    });
    expect(backend.calls.some((c) => c.cmd === "host_create")).toBe(false);

    const h = backend.hosts.find((x) => x.alias === "prod-db2")!;
    expect(useTerminalStore.getState().tabs).toMatchObject([
      {
        hostId: h.id,
        title: "Install key · prod-db2",
        job: { kind: "keyInstall", spec: { hostId: h.id, forgetHostKey: true } },
      },
    ]);
  });

  it("creates the host normally when nothing is checked", async () => {
    const backend = installFakeBackend();
    const user = renderApp();
    act(() => useUiStore.getState().openHostForm("new"));

    await user.type(await screen.findByLabelText("Alias (Host)"), "plain");
    // Nothing to install without an identity file.
    expect(box(/install the public key/i)).toBeDisabled();
    expect(screen.queryByRole("checkbox", { name: /forget/i })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Create host" }));

    await waitFor(() => expect(backend.calls.some((c) => c.cmd === "host_create")).toBe(true));
    expect(backend.calls.some((c) => c.cmd === "host_create_with_key")).toBe(false);
    expect(useTerminalStore.getState().tabs).toEqual([]);
  });

  it("is not offered when editing a host", async () => {
    const backend = installFakeBackend({ hosts: [{ alias: "web", hostname: "10.0.0.1" }] });
    renderApp();
    act(() => useUiStore.getState().openHostForm(backend.hosts[0].id));

    expect(await screen.findByText("Edit web")).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: /generate a new key pair/i })).toBeNull();
  });
});
