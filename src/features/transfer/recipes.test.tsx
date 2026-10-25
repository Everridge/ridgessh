import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProviders } from "@/components/app-providers";
import { useTerminalStore } from "@/features/terminal/store";
import type { FolderRecipe, FolderSpec, Script } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend } from "@/test/fake-backend";
import App from "@/App";
import { initialFromSpec, parseExcludes } from "./folder-plan";

vi.mock("@/features/terminal/terminal-view", () => ({ TerminalView: () => null }));

beforeEach(() => {
  useTerminalStore.setState({ tabs: [], activeId: null });
  useUiStore.setState({ folderTransfer: null, recipesOpen: false });
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

const script: Script = {
  id: "s-restart",
  name: "Restart app",
  description: "",
  content: "systemctl restart app\n",
  interpreter: "bash",
  defaultArgs: "",
  runMode: "stream",
  createdAt: "2026-10-01T00:00:00Z",
  updatedAt: "2026-10-01T00:00:00Z",
};

describe("more options", () => {
  it("parses excludes and restores a saved spec into the dialog", () => {
    expect(parseExcludes("node_modules\r\n\n .git , *.log")).toEqual([
      "node_modules",
      ".git",
      "*.log",
    ]);
    const spec: FolderSpec = {
      source: { kind: "local", path: "C:\\site" },
      destination: { kind: "remote", hostId: "h2", user: "deploy", path: "/var/www" },
      unpack: false,
      keepZip: true,
      existing: "merge",
      owner: "www-data",
      excludes: ["node_modules"],
      sudoDest: true,
      after: { command: "systemctl restart app" },
    };
    expect(initialFromSpec(spec)).toMatchObject({
      mode: "upload",
      hostId: "h2",
      user: "deploy",
      localSrc: "C:\\site",
      dstPath: "/var/www",
      existing: "merge",
      excludes: "node_modules",
      sudoDest: true,
      after: { kind: "command", command: "systemctl restart app" },
      before: { kind: "none" },
    });
  });

  it("adds excludes, sudo and scripts, then saves the transfer as a recipe", async () => {
    const backend = installFakeBackend({
      hosts: [{ alias: "web" }, { alias: "stage" }],
      scripts: [script],
      lastRemote: "/srv/app",
    });
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    useUiStore.getState().openFolderTransfer("copy", backend.hosts[0].id);
    const dialog = await screen.findByRole("dialog", { name: "Copy folder between servers" });

    await user.click(within(dialog).getByRole("combobox", { name: "Destination server" }));
    await user.click(await screen.findByRole("option", { name: /^stage/ }));
    await user.type(within(dialog).getByLabelText("Destination folder on the server"), "/srv");

    await user.click(within(dialog).getByText("More options: leave out, sudo, scripts"));
    await user.type(within(dialog).getByLabelText("Leave out"), "node_modules{enter}*.log");
    await user.click(
      within(dialog).getByRole("checkbox", { name: /read the source folder with sudo/i }),
    );
    await user.click(
      within(dialog).getByRole("combobox", { name: "Script before (on the source server)" }),
    );
    await user.click(await screen.findByRole("option", { name: "A command…" }));
    const start = within(dialog).getByRole("button", { name: "Copy" });
    expect(start).toBeDisabled(); // the command is still empty
    await user.type(
      within(dialog).getByLabelText("Script before (on the source server): command"),
      "pg_dump app > ~/app/db.sql",
    );
    await user.click(
      within(dialog).getByRole("combobox", { name: "Script after (on the destination server)" }),
    );
    await user.click(await screen.findByRole("option", { name: "Script: Restart app" }));

    const plan = within(dialog).getByRole("list", { name: "What will happen" });
    expect(plan).toHaveTextContent("as root into a private temporary file in /tmp, leaving out 2");
    expect(plan).toHaveTextContent("Run the script before on web");
    expect(plan).toHaveTextContent("Run the script after on stage");

    // Save as a recipe; the run is then linked to it.
    await user.click(within(dialog).getByRole("button", { name: /save as recipe/i }));
    await user.type(within(dialog).getByLabelText("Recipe name"), "prod → stage");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await screen.findByText('Saved as "prod → stage"')).toBeInTheDocument();
    const saved = backend.calls.find((c) => c.cmd === "recipe_save")!;
    expect(saved.args.spec).toMatchObject({
      excludes: ["node_modules", "*.log"],
      sudoSource: true,
      sudoDest: false,
      before: { command: "pg_dump app > ~/app/db.sql" },
      after: { scriptId: "s-restart" },
    });

    await user.click(start);
    const job = useTerminalStore.getState().tabs[0].job;
    expect(job).toMatchObject({ kind: "folder", recipeId: expect.any(String) });
  });
});

describe("recipes list", () => {
  const recipe = (hostId: string, over: Partial<FolderRecipe> = {}): FolderRecipe => ({
    id: "r1",
    name: "Nightly uploads",
    spec: {
      source: { kind: "remote", hostId, path: "/srv/uploads" },
      destination: { kind: "local", path: "C:\\backup" },
      unpack: false,
      keepZip: true,
      existing: "backup",
    },
    createdAt: "2026-10-01T00:00:00Z",
    updatedAt: "2026-10-01T00:00:00Z",
    lastRunAt: "2026-10-24T22:00:00Z",
    lastExitCode: 0,
    ...over,
  });

  it("runs a recipe as saved, opens one for changes, and deletes one", async () => {
    const backend = installFakeBackend({ hosts: [{ alias: "web" }] });
    const hostId = backend.hosts[0].id;
    backend.recipes.push(
      recipe(hostId),
      recipe("gone", { id: "r2", name: "Old server", lastExitCode: 1 }),
    );
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    await user.click(screen.getByRole("button", { name: "Folder recipes" }));

    const dialog = await screen.findByRole("dialog", { name: "Folder recipes" });
    const list = await within(dialog).findByRole("list", { name: "Recipes" });
    expect(within(list).getByText(/^Done /)).toBeInTheDocument();
    expect(within(list).getByText(/^Failed /)).toBeInTheDocument();
    // A recipe whose server was deleted cannot run.
    expect(within(list).getByRole("button", { name: "Run Old server" })).toBeDisabled();

    await user.click(within(list).getByRole("button", { name: "Run Nightly uploads" }));
    expect(useTerminalStore.getState().tabs[0]).toMatchObject({
      title: "⇣ uploads ← web",
      job: { kind: "folder", recipeId: "r1", spec: { source: { path: "/srv/uploads" } } },
    });

    // Open: the transfer dialog with the recipe's settings.
    await user.click(screen.getByRole("button", { name: "Folder recipes" }));
    await user.click(await screen.findByRole("button", { name: "Open Nightly uploads" }));
    const folder = await screen.findByRole("dialog", {
      name: "Download folder: Nightly uploads",
    });
    expect(within(folder).getByLabelText("Remote folder")).toHaveValue("/srv/uploads");
    expect(within(folder).getByLabelText("Save to")).toHaveValue("C:\\backup");
    await user.click(within(folder).getByRole("button", { name: "Cancel" }));

    await user.click(screen.getByRole("button", { name: "Folder recipes" }));
    await user.click(await screen.findByRole("button", { name: "Delete Old server" }));
    await user.click(await screen.findByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Run Old server" })).not.toBeInTheDocument(),
    );
  });
});

describe("remote folder browser", () => {
  it("navigates folders and fills in the chosen path", async () => {
    const backend = installFakeBackend({
      hosts: [{ alias: "web" }],
      remoteTree: {
        "/home/me": [
          { name: "sites", dir: true },
          { name: ".bashrc", dir: false },
        ],
        "/home/me/sites": [{ name: "shop", dir: true }],
        "/home/me/sites/shop": [],
      },
    });
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    useUiStore.getState().openFolderTransfer("download", backend.hosts[0].id);
    const dialog = await screen.findByRole("dialog", { name: "Download folder" });
    await user.click(within(dialog).getByRole("button", { name: "Browse: Remote folder" }));

    const browser = await screen.findByRole("dialog", { name: "Choose the folder to copy" });
    await user.click(await within(browser).findByRole("button", { name: "sites" }));
    await user.click(await within(browser).findByRole("button", { name: "shop" }));
    expect(await within(browser).findByText("Empty folder")).toBeInTheDocument();
    await user.click(within(browser).getByRole("button", { name: "Choose this folder" }));

    await waitFor(() =>
      expect(within(dialog).getByLabelText("Remote folder")).toHaveValue("/home/me/sites/shop"),
    );
  });
});
