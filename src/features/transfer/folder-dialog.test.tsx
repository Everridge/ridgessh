import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProviders } from "@/components/app-providers";
import { useTerminalStore } from "@/features/terminal/store";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend } from "@/test/fake-backend";
import App from "@/App";

vi.mock("@/features/terminal/terminal-view", () => ({ TerminalView: () => null }));

beforeEach(() => {
  useTerminalStore.setState({ tabs: [], activeId: null });
  useUiStore.setState({ folderTransfer: null });
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

describe("download folder", () => {
  it("starts a zip transfer tab with the chosen options", async () => {
    const backend = installFakeBackend({
      hosts: [{ alias: "web" }],
      lastRemote: "/srv/app/",
      dialogResult: "C:\\Downloads",
    });
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    useUiStore.getState().openFolderTransfer(backend.hosts[0].id);

    const dialog = await screen.findByRole("dialog", { name: "Download folder — web" });
    const download = within(dialog).getByRole("button", { name: "Download" });
    // The last remote folder is suggested; a local folder still has to be chosen.
    await waitFor(() =>
      expect(within(dialog).getByLabelText("Remote folder")).toHaveValue("/srv/app/"),
    );
    expect(download).toBeDisabled();
    await user.click(within(dialog).getByRole("button", { name: /choose/i }));
    expect(within(dialog).getByLabelText("Save to")).toHaveValue("C:\\Downloads");

    await user.click(within(dialog).getByRole("checkbox", { name: "Unpack after downloading" }));
    await user.click(within(dialog).getByRole("checkbox", { name: "Keep the .zip as well" }));
    const plan = within(dialog).getByRole("list", { name: "What will happen" });
    expect(plan).toHaveTextContent("delete the .zip");
    await user.click(download);

    const tab = useTerminalStore.getState().tabs[0];
    expect(tab.title).toBe("⇣ app ← web");
    expect(tab.job).toEqual({
      kind: "folder",
      spec: {
        hostId: backend.hosts[0].id,
        user: null,
        remotePath: "/srv/app/",
        localDir: "C:\\Downloads",
        unpack: true,
        keepZip: false,
      },
    });
  });

  it("shows the steps of a running transfer and offers Retry after a failure", async () => {
    installFakeBackend({ hosts: [{ alias: "web" }] });
    renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    const s = useTerminalStore.getState();
    const id = s.openJob(
      { id: "h1", alias: "web" },
      {
        kind: "folder",
        spec: { hostId: "h1", remotePath: "/a", localDir: "C:\\x", unpack: false, keepZip: true },
      },
      "⇣ a ← web",
    );
    s.setRun(id, "run1", ["Check", "Zip on server", "Download", "Clean up"]);
    s.setStep(id, 0, "done", null);
    s.setStep(id, 1, "warning", "Some files could not be read");
    s.setStep(id, 2, "failed", "scp failed");
    s.setStatus(id, "exited", 1);

    const steps = await screen.findByRole("list", { name: "Steps" });
    expect(within(steps).getByLabelText("Download: failed")).toHaveAttribute("title", "scp failed");
    expect(within(steps).getByLabelText("Clean up: pending")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });
});
