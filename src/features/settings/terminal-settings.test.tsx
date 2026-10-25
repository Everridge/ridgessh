import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProviders } from "@/components/app-providers";
import { fontStack, resolveScheme, TERMINAL_SCHEMES } from "@/features/terminal/themes";
import { useUiStore } from "@/stores/ui";
import { installFakeBackend } from "@/test/fake-backend";
import App from "@/App";

vi.mock("@/features/terminal/terminal-view", () => ({ TerminalView: () => null }));

beforeEach(() => useUiStore.setState({ settingsOpen: false }));

describe("terminal schemes", () => {
  it("have unique ids and a full 16-colour palette", () => {
    const ids = TERMINAL_SCHEMES.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const s of TERMINAL_SCHEMES) {
      expect(s.id).toMatch(/^[a-z0-9-]+$/); // what the backend accepts
      for (const key of ["background", "foreground", "red", "brightRed", "brightWhite"] as const)
        expect(s.theme[key], `${s.id}.${key}`).toMatch(/^#[0-9a-f]{6}$/);
    }
  });

  it("fall back to the app's light/dark mode for auto and unknown ids", () => {
    expect(resolveScheme("auto", true).theme.background).toBe("#0a0a0a");
    expect(resolveScheme("gone", false).theme.background).toBe("#ffffff");
    expect(resolveScheme("nord", false).name).toBe("Nord");
    expect(fontStack(" Fira Code ")).toMatch(/^"Fira Code", /);
    expect(fontStack(null)).not.toContain('""');
  });
});

describe("terminal settings", () => {
  it("previews and saves a scheme, font and size", async () => {
    const backend = installFakeBackend({});
    const user = userEvent.setup();
    render(
      <AppProviders>
        <App />
      </AppProviders>,
    );
    useUiStore.getState().setSettingsOpen(true);
    const dialog = await screen.findByRole("dialog", { name: "Settings" });
    await user.click(within(dialog).getByRole("tab", { name: "Terminal" }));

    await user.click(within(dialog).getByRole("combobox", { name: "Colour scheme" }));
    await user.click(await screen.findByRole("option", { name: "Dracula" }));
    const preview = within(dialog).getByRole("img", { name: "Preview of Dracula" });
    expect(preview.style.background).toBe("rgb(40, 42, 54)");

    await user.type(within(dialog).getByLabelText("Font"), "Fira Code");
    const size = within(dialog).getByLabelText("Size");
    await user.clear(size);
    await user.type(size, "16");
    expect(preview).toHaveStyle({ fontSize: "16px" });

    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(backend.calls.find((c) => c.cmd === "settings_update")?.args.settings).toMatchObject({
        terminalTheme: "dracula",
        terminalFontFamily: "Fira Code",
        terminalFontSize: 16,
      }),
    );
  });
});
