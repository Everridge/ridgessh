/** True in the macOS build (WKWebView reports a Mac platform). */
export const isMac = /Mac/i.test(globalThis.navigator?.platform || globalThis.navigator?.userAgent);

/** Platform words for UI text. */
export const platform = {
  /** "Show in Finder" / "Show in Explorer". */
  fileManager: isMac ? "Finder" : "Explorer",
  /** Where the backup password is kept. */
  keychain: isMac ? "the macOS Keychain" : "Windows Credential Manager",
  /** Shortcut label: "⌘K" or "Ctrl+K". */
  shortcut: (key: string) => (isMac ? `⌘${key}` : `Ctrl+${key}`),
};

/** The app's modifier: Cmd on macOS, Ctrl elsewhere. */
export const modKey = (e: { metaKey: boolean; ctrlKey: boolean }) =>
  isMac ? e.metaKey : e.ctrlKey;
