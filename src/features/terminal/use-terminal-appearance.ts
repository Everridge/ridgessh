import { useTheme } from "@/components/theme-provider";
import { useSettings } from "@/features/backup/queries";
import { AUTO_SCHEME_ID, fontStack, resolveScheme } from "./themes";

/** The colour scheme and font the terminal tabs use, from Settings. */
export function useTerminalAppearance() {
  const { resolvedTheme } = useTheme();
  const s = useSettings().data?.settings;
  const scheme = resolveScheme(s?.terminalTheme ?? AUTO_SCHEME_ID, resolvedTheme === "dark");
  return {
    scheme,
    fontFamily: fontStack(s?.terminalFontFamily),
    fontSize: s?.terminalFontSize ?? 13,
  };
}
