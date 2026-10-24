import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProviders } from "@/components/app-providers";
import { useTerminalStore } from "@/features/terminal/store";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend } from "@/test/fake-backend";
import App from "@/App";
import { useUpdateStore } from "./store";
import { useStartupUpdateCheck } from "./use-startup-update-check";

const updater = vi.hoisted(() => ({
  check: vi.fn(),
  relaunch: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: updater.check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: updater.relaunch }));
vi.mock("@/features/terminal/terminal-view", () => ({ TerminalView: () => null }));

type Progress = (e: { event: string; data?: Record<string, number> }) => void;

function fakeUpdate() {
  return {
    version: "0.2.0",
    date: "2026-11-01T10:00:00Z",
    body: "Faster startup.\nFixed import of large files.",
    downloadAndInstall: vi.fn(async (onEvent: Progress) => {
      onEvent({ event: "Started", data: { contentLength: 2048 } });
      onEvent({ event: "Progress", data: { chunkLength: 2048 } });
      onEvent({ event: "Finished" });
    }),
  };
}

beforeEach(() => {
  updater.check.mockReset();
  updater.relaunch.mockReset();
  useUpdateStore.setState({
    status: "idle",
    update: null,
    downloaded: 0,
    total: null,
    error: null,
    dialogOpen: false,
  });
  useTerminalStore.setState({ tabs: [], activeId: null });
  useUiStore.setState({ settingsOpen: false });
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

describe("updates", () => {
  it("checks from settings, shows the notes and installs", async () => {
    installFakeBackend({ hosts: [{ alias: "web" }] });
    const update = fakeUpdate();
    updater.check.mockResolvedValue(update);
    const user = renderApp();
    useTerminalStore.getState().open({ id: "h1", alias: "web" });
    useUiStore.getState().setSettingsOpen(true);

    const settings = await screen.findByRole("dialog", { name: "Settings" });
    await user.click(within(settings).getByRole("tab", { name: "About" }));
    expect(within(settings).getByRole("checkbox", { name: /check for updates/i })).toBeChecked();
    await user.click(within(settings).getByRole("button", { name: "Check now" }));

    const dialog = await screen.findByRole("dialog", { name: "RidgeSSH 0.2.0" });
    expect(within(dialog).getByText(/Fixed import of large files/)).toBeInTheDocument();
    expect(within(dialog).getByText(/1 open terminal session will be closed/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: /install and restart/i }));

    await waitFor(() => expect(updater.relaunch).toHaveBeenCalled());
    expect(update.downloadAndInstall).toHaveBeenCalledOnce();
    expect(useUpdateStore.getState()).toMatchObject({ status: "installing", downloaded: 2048 });
  });

  it("says when the app is up to date and shows check errors", async () => {
    installFakeBackend({});
    updater.check.mockResolvedValueOnce(null).mockRejectedValueOnce("network unreachable");
    const user = renderApp();
    useUiStore.getState().setSettingsOpen(true);
    const settings = await screen.findByRole("dialog", { name: "Settings" });
    await user.click(within(settings).getByRole("tab", { name: "About" }));

    await user.click(within(settings).getByRole("button", { name: "Check now" }));
    expect(await within(settings).findByText("RidgeSSH is up to date.")).toBeInTheDocument();
    await user.click(within(settings).getByRole("button", { name: "Check now" }));
    expect(await within(settings).findByText("network unreachable")).toBeInTheDocument();
  });

  it("checks once on startup and offers the update in a notification", async () => {
    installFakeBackend({});
    updater.check.mockResolvedValue(fakeUpdate());
    function Probe() {
      useStartupUpdateCheck(true, 0);
      return null;
    }
    const user = userEvent.setup();
    render(
      <AppProviders>
        <Probe />
      </AppProviders>,
    );
    expect(await screen.findByText("RidgeSSH 0.2.0 is available")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "View" }));
    expect(useUpdateStore.getState().dialogOpen).toBe(true);
    expect(updater.check).toHaveBeenCalledOnce();
  });

  it("does not check on startup when the setting is off", async () => {
    installFakeBackend({ settings: { checkUpdates: false } });
    function Probe() {
      useStartupUpdateCheck(true, 0);
      return null;
    }
    render(
      <AppProviders>
        <Probe />
      </AppProviders>,
    );
    await new Promise((r) => setTimeout(r, 50));
    expect(updater.check).not.toHaveBeenCalled();
  });
});
