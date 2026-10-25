import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProviders } from "@/components/app-providers";
import { useTerminalStore } from "@/features/terminal/store";
import type { FolderSpec } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend } from "@/test/fake-backend";
import App from "@/App";
import { folderName, planSteps, tabTitle, validOwner } from "./folder-plan";

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

describe("folder plan helpers", () => {
  const alias = (id: string) => ({ a: "web", b: "db" })[id] ?? id;
  const base = { unpack: false, keepZip: true, existing: "backup" } as const;

  it("names folders and tabs", () => {
    expect(folderName("/srv/app/")).toBe("app");
    expect(folderName("C:\\work\\site")).toBe("site");
    expect(folderName("~")).toBe("");
    const copy: FolderSpec = {
      ...base,
      source: { kind: "remote", hostId: "a", user: "deploy", path: "/srv/app" },
      destination: { kind: "remote", hostId: "b", path: "/srv" },
    };
    expect(tabTitle(copy, alias)).toBe("⇄ app: deploy@web → db");
  });

  it("lists the steps for each direction", () => {
    const up: FolderSpec = {
      ...base,
      owner: "www-data",
      source: { kind: "local", path: "C:\\site" },
      destination: { kind: "remote", hostId: "a", path: "/var/www" },
    };
    const steps = planSteps(up, alias);
    expect(steps).toHaveLength(7);
    expect(steps[2]).toBe("Zip on this PC into a temporary file");
    expect(steps[5]).toBe("Set the owner to www-data");
    expect(validOwner("www-data:www-data")).toBe(true);
    expect(validOwner("a b")).toBe(false);
    expect(validOwner("-R")).toBe(false);

    const direct: FolderSpec = {
      ...base,
      direct: true,
      source: { kind: "remote", hostId: "a", path: "/srv/app" },
      destination: { kind: "remote", hostId: "b", path: "/srv" },
    };
    const streamed = planSteps(direct, alias);
    expect(streamed).toHaveLength(6);
    expect(streamed[3]).toBe(
      "Stream from web to db through this app (nothing is saved here), compare SHA-256 checksums",
    );
    expect(streamed.some((s) => s.startsWith("Upload"))).toBe(false);
  });
});

describe("folder dialog", () => {
  it("downloads a folder with the chosen options", async () => {
    const backend = installFakeBackend({
      hosts: [{ alias: "web" }],
      lastRemote: "/srv/app/",
      dialogResult: "C:\\Downloads",
    });
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    useUiStore.getState().openFolderTransfer("download", backend.hosts[0].id);

    const dialog = await screen.findByRole("dialog", { name: "Download folder" });
    const start = within(dialog).getByRole("button", { name: "Download" });
    // The last remote folder is suggested; a local folder still has to be chosen.
    await waitFor(() =>
      expect(within(dialog).getByLabelText("Remote folder")).toHaveValue("/srv/app/"),
    );
    expect(start).toBeDisabled();
    await user.click(within(dialog).getByRole("button", { name: "Choose: Save to" }));
    await user.click(within(dialog).getByRole("checkbox", { name: "Unpack after downloading" }));
    await user.click(within(dialog).getByRole("checkbox", { name: "Keep the .zip as well" }));
    expect(within(dialog).getByRole("list", { name: "What will happen" })).toHaveTextContent(
      "delete the .zip",
    );
    await user.click(start);

    const tab = useTerminalStore.getState().tabs[0];
    expect(tab.title).toBe("⇣ app ← web");
    expect(tab.job).toEqual({
      kind: "folder",
      recipeId: null,
      spec: {
        source: { kind: "remote", hostId: backend.hosts[0].id, user: null, path: "/srv/app/" },
        destination: { kind: "local", path: "C:\\Downloads" },
        unpack: true,
        keepZip: false,
        keepCopy: null,
        direct: false,
        existing: "backup",
        owner: null,
        excludes: [],
        sudoSource: false,
        sudoDest: false,
        before: null,
        after: null,
      },
    });
  });

  it("uploads a local folder into a server folder, with an owner", async () => {
    const backend = installFakeBackend({
      hosts: [{ alias: "web" }],
      dialogResult: "C:\\work\\site",
    });
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    useUiStore.getState().openFolderTransfer("upload", backend.hosts[0].id);

    const dialog = await screen.findByRole("dialog", { name: "Upload folder" });
    await user.click(within(dialog).getByRole("button", { name: "Choose: Folder on this PC" }));
    await user.type(within(dialog).getByLabelText("Destination folder on the server"), "/var/www");
    expect(within(dialog).getByText("The folder ends up as /var/www/site")).toBeInTheDocument();

    const owner = within(dialog).getByLabelText("Set owner (optional)");
    await user.type(owner, "bad owner");
    expect(within(dialog).getByRole("button", { name: "Upload" })).toBeDisabled();
    await user.clear(owner);
    await user.type(owner, "www-data:www-data");
    await user.click(within(dialog).getByRole("button", { name: "Upload" }));

    const tab = useTerminalStore.getState().tabs[0];
    expect(tab.title).toBe("⇡ site → web");
    expect(tab.job).toMatchObject({
      kind: "folder",
      spec: {
        source: { kind: "local", path: "C:\\work\\site" },
        destination: { kind: "remote", hostId: backend.hosts[0].id, path: "/var/www" },
        owner: "www-data:www-data",
        existing: "backup",
      },
    });
  });

  it("copies between two servers", async () => {
    const backend = installFakeBackend({
      hosts: [{ alias: "web" }, { alias: "db" }],
      lastRemote: "/srv/app",
    });
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    useUiStore.getState().openFolderTransfer("copy", backend.hosts[0].id);

    const dialog = await screen.findByRole("dialog", { name: "Copy folder between servers" });
    const start = within(dialog).getByRole("button", { name: "Copy" });
    expect(start).toBeDisabled();
    await user.click(within(dialog).getByRole("combobox", { name: "Destination server" }));
    await user.click(await screen.findByRole("option", { name: /^db/ }));
    await user.type(within(dialog).getByLabelText("Destination folder on the server"), "/srv");
    await user.click(within(dialog).getByRole("combobox", { name: /if a folder of that name/i }));
    await user.click(await screen.findByRole("option", { name: /stop without/i }));
    await waitFor(() => expect(start).toBeEnabled());
    await user.click(start);

    const tab = useTerminalStore.getState().tabs[0];
    expect(tab.title).toBe("⇄ app: web → db");
    expect(tab.job).toMatchObject({
      spec: {
        source: { kind: "remote", hostId: backend.hosts[0].id, path: "/srv/app" },
        destination: { kind: "remote", hostId: backend.hosts[1].id, path: "/srv" },
        existing: "stop",
      },
    });
  });

  it("streams directly between two servers", async () => {
    const backend = installFakeBackend({
      hosts: [{ alias: "web" }, { alias: "db" }],
      lastRemote: "/srv/app",
    });
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    useUiStore.getState().openFolderTransfer("copy", backend.hosts[0].id);

    const dialog = await screen.findByRole("dialog", { name: "Copy folder between servers" });
    await user.click(within(dialog).getByRole("combobox", { name: "Destination server" }));
    await user.click(await screen.findByRole("option", { name: /^db/ }));
    await user.type(within(dialog).getByLabelText("Destination folder on the server"), "/srv");
    await user.click(
      within(dialog).getByRole("checkbox", { name: "Also keep the archive on this PC" }),
    );
    await user.click(
      within(dialog).getByRole("checkbox", { name: /stream directly between the servers/i }),
    );
    // Nothing is stored here, so there is no local copy to keep.
    expect(
      within(dialog).queryByRole("checkbox", { name: "Also keep the archive on this PC" }),
    ).not.toBeInTheDocument();
    const plan = within(dialog).getByRole("list", { name: "What will happen" });
    expect(plan).toHaveTextContent("Stream from web to db through this app");
    expect(plan).not.toHaveTextContent("Upload to db");
    await user.click(within(dialog).getByRole("button", { name: "Copy" }));

    expect(useTerminalStore.getState().tabs[0].job).toMatchObject({
      spec: { direct: true, keepCopy: null },
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
        spec: {
          source: { kind: "remote", hostId: "h1", path: "/a" },
          destination: { kind: "local", path: "C:\\x" },
          unpack: false,
          keepZip: true,
          existing: "backup",
        },
      },
      "⇣ a ← web",
    );
    s.setRun(id, "run1", ["Check source", "Zip on server", "Download", "Clean up"]);
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
