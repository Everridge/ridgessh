import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProviders } from "@/components/app-providers";
import type { ImportPreview } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend } from "@/test/fake-backend";
import App from "@/App";
import { strategiesFor, summarize } from "./import-plan";

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

const preview: ImportPreview = {
  kind: "export",
  fileName: "team.rsexport",
  createdAt: "2026-10-20T08:00:00.000Z",
  appVersion: "0.1.0",
  hosts: [
    {
      key: "x1",
      alias: "fresh",
      hostname: "fresh.example.com",
      user: "ops",
      port: 22,
      groupPath: ["Prod"],
      status: "new",
      existingId: null,
      differences: [],
      copyAlias: "fresh-2",
      suggested: "import",
      note: null,
    },
    {
      key: "x2",
      alias: "web",
      hostname: "web.example.com",
      user: null,
      port: 2222,
      groupPath: [],
      status: "differs",
      existingId: "h-web",
      differences: ["port"],
      copyAlias: "web-2",
      suggested: "skip",
      note: null,
    },
  ],
  scripts: [],
  keys: [],
  warnings: [],
};

beforeEach(() => {
  useUiStore.setState({
    view: "all",
    selectedHostId: null,
    checkedIds: [],
    exportTarget: null,
    importOpen: false,
    configWrite: null,
  });
});

describe("import plan helpers", () => {
  it("offers import only for new items and counts what will happen", () => {
    expect(strategiesFor("new")).toEqual(["import", "skip"]);
    expect(strategiesFor("differs")).toEqual(["skip", "overwrite", "copy"]);
    expect(summarize({ a: "import", b: "skip" }, { s: "copy" })).toEqual({
      count: 2,
      text: "1 host, 1 script will be imported",
    });
    expect(summarize({ a: "skip" }, {}).text).toBe("Nothing selected");
  });
});

describe("export", () => {
  it("exports the selected hosts encrypted with a confirmed password", async () => {
    const backend = installFakeBackend({
      hosts: [{ alias: "web" }, { alias: "db" }],
      saveResult: "C:\\exports\\team.rsexport",
    });
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    useUiStore.getState().openExport([backend.hosts[0].id]);

    const dialog = await screen.findByRole("dialog", { name: "Export" });
    expect(within(dialog).getByText("Hosts (1 of 2)")).toBeInTheDocument();
    const exportBtn = within(dialog).getByRole("button", { name: /export…/i });
    expect(exportBtn).toBeDisabled();

    await user.type(within(dialog).getByLabelText("Export password"), "long enough");
    await user.type(within(dialog).getByLabelText("Repeat password"), "long enough");
    await user.click(exportBtn);

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    const call = backend.calls.find((c) => c.cmd === "export_create")!;
    expect(call.args).toMatchObject({
      path: "C:\\exports\\team.rsexport",
      password: "long enough",
      selection: { hostIds: [backend.hosts[0].id], scriptIds: [], includeKeys: false },
    });
    expect(await screen.findByText("Exported 1 host")).toBeInTheDocument();
  });

  it("writes a plain config without asking for a password", async () => {
    const backend = installFakeBackend({
      hosts: [{ alias: "web" }],
      saveResult: "C:\\exports\\ssh_config",
    });
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    useUiStore.getState().openExport();
    const dialog = await screen.findByRole("dialog", { name: "Export" });
    await user.click(within(dialog).getByRole("tab", { name: "Plain ssh config" }));
    expect(within(dialog).queryByLabelText("Export password")).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: /export…/i }));
    await waitFor(() =>
      expect(backend.calls.find((c) => c.cmd === "export_config_file")?.args).toMatchObject({
        path: "C:\\exports\\ssh_config",
        hostIds: [backend.hosts[0].id],
      }),
    );
  });
});

describe("import", () => {
  it("previews, lets the user choose per host, imports and offers the config write", async () => {
    const backend = installFakeBackend({
      hosts: [{ alias: "web" }],
      dialogResult: "C:\\in\\team.rsexport",
      importPreview: preview,
      importPassword: "secret pw",
    });
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    useUiStore.getState().setImportOpen(true);

    let dialog = await screen.findByRole("dialog", { name: "Import" });
    await user.click(within(dialog).getByRole("button", { name: /choose/i }));
    await user.type(await within(dialog).findByLabelText("Export password"), "wrong pw");
    await user.click(within(dialog).getByRole("button", { name: "Open" }));
    expect(await screen.findByText("wrong password for this export")).toBeInTheDocument();

    await user.clear(within(dialog).getByLabelText("Export password"));
    await user.type(within(dialog).getByLabelText("Export password"), "secret pw");
    await user.click(within(dialog).getByRole("button", { name: "Open" }));

    dialog = await screen.findByRole("dialog", { name: "Import from team.rsexport" });
    const table = within(dialog).getByRole("table", { name: "Hosts to import" });
    expect(within(table).getByText("Differs")).toHaveAttribute("title", "Differs in: port");
    expect(within(dialog).getByText("1 host will be imported")).toBeInTheDocument();

    // The existing host: import it as a copy instead of skipping it.
    await user.click(within(dialog).getByRole("combobox", { name: "Action for web" }));
    await user.click(await screen.findByRole("option", { name: "Copy as web-2" }));
    expect(within(dialog).getByText("2 hosts will be imported")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: /^import$/i }));

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    const apply = backend.calls.find((c) => c.cmd === "import_apply")!;
    expect(apply.args.request).toMatchObject({
      targetGroupId: null,
      hosts: [
        { key: "x1", strategy: "import" },
        { key: "x2", strategy: "copy" },
      ],
      installKeys: false,
    });
    expect(await screen.findByText("Hosts: 1 new, 1 copied")).toBeInTheDocument();
    expect(await screen.findByText("web-2")).toBeInTheDocument();

    // "Review" opens the config write dialog focused on the imported hosts.
    await user.click(screen.getByRole("button", { name: "Review" }));
    const added = backend.hosts.filter((h) => h.alias === "fresh" || h.alias === "web-2");
    expect(useUiStore.getState().configWrite?.focus?.sort()).toEqual(added.map((h) => h.id).sort());
  });

  it("reads a plain config without a password", async () => {
    const backend = installFakeBackend({
      hosts: [{ alias: "web" }],
      dialogResult: "C:\\in\\config",
      importPreview: { ...preview, hosts: [preview.hosts[0]] },
    });
    const user = renderApp();
    await screen.findByRole("list", { name: "Hosts" });
    useUiStore.getState().setImportOpen(true);
    const dialog = await screen.findByRole("dialog", { name: "Import" });
    await user.click(within(dialog).getByRole("button", { name: /choose/i }));
    expect(
      await screen.findByText(/Plain ssh config: only concrete Host blocks/),
    ).toBeInTheDocument();
    expect(backend.calls.find((c) => c.cmd === "import_preview")?.args.source).toEqual({
      kind: "config",
      path: "C:\\in\\config",
    });
  });
});
