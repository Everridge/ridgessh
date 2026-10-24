import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { AppProviders } from "@/components/app-providers";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend } from "@/test/fake-backend";
import App from "@/App";

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
    selectedHostId: null,
    checkedIds: [],
    hostForm: null,
    syncReviewOpen: false,
    configWrite: null,
    settingsOpen: false,
    backupsOpen: false,
    restoreTarget: null,
  });
});

const dialogNamed = (name: RegExp) => screen.findByRole("dialog", { name });

describe("backups", () => {
  it("warns without a password and asks for one for a manual backup", async () => {
    const backend = installFakeBackend({ hosts: [{ alias: "web" }] });
    const user = renderApp();

    expect(
      await screen.findByLabelText(/automatic backups are off: no backup password/i),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /^backups/i }));
    const dialog = await dialogNamed(/^backups$/i);
    expect(within(dialog).getByText(/no backup password is remembered/i)).toBeInTheDocument();
    expect(within(dialog).getByText(/no backups yet/i)).toBeInTheDocument();

    await user.click(within(dialog).getByRole("button", { name: /back up now/i }));
    const prompt = await dialogNamed(/backup password/i);
    const submit = within(prompt).getByRole("button", { name: /^back up$/i });
    await user.type(within(prompt).getByLabelText(/^backup password$/i), "short");
    expect(within(prompt).getByText(/at least 8 characters/i)).toBeInTheDocument();
    await user.type(within(prompt).getByLabelText(/^backup password$/i), " but longer");
    await user.type(within(prompt).getByLabelText(/repeat password/i), "short but longer");
    await waitFor(() => expect(submit).toBeEnabled());
    await user.click(submit);

    expect(await within(dialog).findByRole("list", { name: /backup files/i })).toBeInTheDocument();
    expect(backend.calls.find((c) => c.cmd === "backup_password_remember")?.args).toEqual({
      password: "short but longer",
    });
    expect(backend.calls.find((c) => c.cmd === "backup_create")?.args).toEqual({
      password: "short but longer",
    });
    await waitFor(() =>
      expect(within(dialog).queryByText(/no backup password is remembered/i)).toBeNull(),
    );
  });

  it("restores after the password, a preview and the confirmation word", async () => {
    const backend = installFakeBackend({
      hosts: [{ alias: "web" }],
      rememberedPassword: "remembered pw",
      backups: [{ fileName: "ridgessh-old-manual.rsbak", password: "old password" }],
    });
    const user = renderApp();
    await user.click(await screen.findByRole("button", { name: /^backups/i }));
    const list = await within(await dialogNamed(/^backups$/i)).findByRole("list", {
      name: /backup files/i,
    });
    await user.click(within(list).getByRole("button", { name: /restore/i }));

    const dialog = await dialogNamed(/restore backup/i);
    const pw = within(dialog).getByLabelText(/password of this backup/i);
    await user.type(pw, "wrong one{Enter}");
    expect(await within(dialog).findByText(/wrong password/i)).toBeInTheDocument();
    await user.clear(pw);
    await user.type(pw, "old password{Enter}");

    expect(await within(dialog).findByText("old-box")).toBeInTheDocument();
    expect(within(dialog).getByText("Host old-box")).toBeInTheDocument();
    const restore = within(dialog).getByRole("button", { name: /^restore$/i });
    expect(restore).toBeEnabled(); // database only, the default

    await user.click(within(dialog).getByRole("checkbox", { name: /restore c:/i }));
    expect(restore).toBeDisabled();
    await user.type(within(dialog).getByLabelText(/to replace the ssh config/i), "CONFIRM");
    await waitFor(() => expect(restore).toBeEnabled());
    await user.click(restore);

    expect(await screen.findByText(/restored the backup from/i)).toBeInTheDocument();
    expect(backend.calls.find((c) => c.cmd === "backup_restore")?.args).toEqual({
      path: "C:\\data\\backups\\ridgessh-old-manual.rsbak",
      password: "old password",
      options: {
        restoreDb: true,
        restoreConfig: true,
        configBaseHash: "cfg-hash",
        confirmation: "CONFIRM",
      },
    });
  });

  it("saves settings", async () => {
    const backend = installFakeBackend({ rememberedPassword: "remembered pw" });
    const user = renderApp();
    await user.click(await screen.findByRole("button", { name: /^settings/i }));
    const dialog = await dialogNamed(/^settings$/i);
    expect(
      await within(dialog).findByText(/remembered in windows credential manager/i),
    ).toBeInTheDocument();

    await user.click(within(dialog).getByRole("tab", { name: /config & trash/i }));
    const word = within(dialog).getByLabelText(/confirmation word/i);
    await user.clear(word);
    await user.type(word, "OVERWRITE");
    await user.click(within(dialog).getByRole("button", { name: /^save$/i }));

    expect(await screen.findByText(/settings saved/i)).toBeInTheDocument();
    const saved = backend.calls.find((c) => c.cmd === "settings_update")?.args;
    expect(saved).toMatchObject({ settings: { confirmationWord: "OVERWRITE", autoBackup: true } });
  });
});
