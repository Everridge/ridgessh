import { useId, useState } from "react";
import { useTheme } from "@/components/theme-provider";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  AUTO_SCHEME_ID,
  FONT_SUGGESTIONS,
  TERMINAL_SCHEMES,
  fontStack,
  resolveScheme,
  type TerminalScheme,
} from "@/features/terminal/themes";
import type { Settings } from "@/lib/ipc";

type Set = <K extends keyof Settings>(k: K, v: Settings[K]) => void;

const ANSI = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"] as const;

/** Colour scheme, font and size of the terminal tabs, with a live preview. */
export function TerminalSettings({ s, set }: { s: Settings; set: Set }) {
  const { resolvedTheme } = useTheme();
  const scheme = resolveScheme(s.terminalTheme, resolvedTheme === "dark");
  const fontId = useId();
  const sizeId = useId();
  const listId = useId();

  return (
    <div className="grid gap-4">
      <div className="grid gap-1.5">
        <Label>Colour scheme</Label>
        <Select value={s.terminalTheme} onValueChange={(v) => set("terminalTheme", v)}>
          <SelectTrigger aria-label="Colour scheme" className="w-full sm:w-72">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={AUTO_SCHEME_ID}>Auto (follows light / dark mode)</SelectItem>
            {TERMINAL_SCHEMES.map((sc) => (
              <SelectItem key={sc.id} value={sc.id}>
                <Swatches scheme={sc} />
                {sc.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="grid gap-3 sm:grid-cols-[1fr_8rem]">
        <div className="grid gap-1.5">
          <Label htmlFor={fontId}>Font</Label>
          <Input
            id={fontId}
            list={listId}
            value={s.terminalFontFamily ?? ""}
            onChange={(e) => set("terminalFontFamily", e.target.value || null)}
            placeholder="Default monospace font"
            spellCheck={false}
          />
          <datalist id={listId}>
            {FONT_SUGGESTIONS.map((f) => (
              <option key={f} value={f} />
            ))}
          </datalist>
          <p className="text-xs text-muted-foreground">
            Any installed font. If it is missing, the default is used.
          </p>
        </div>
        <div className="grid content-start gap-1.5">
          <Label htmlFor={sizeId}>Size</Label>
          <SizeInput
            id={sizeId}
            value={s.terminalFontSize}
            onChange={(v) => set("terminalFontSize", v)}
          />
        </div>
      </div>

      <Preview scheme={scheme} family={s.terminalFontFamily} size={s.terminalFontSize} />
      <p className="text-xs text-muted-foreground">
        The scheme sets how colours look. Which colours appear is up to the server: a plain prompt
        usually means the remote shell has colours turned off (for example in /root/.bashrc).
      </p>
    </div>
  );
}

/**
 * Font size 8–32. Typing is free ("1" on the way to "16" is fine); only valid sizes are
 * applied, and leaving the field shows the applied size again.
 */
function SizeInput({
  id,
  value,
  onChange,
}: {
  id: string;
  value: number;
  onChange: (v: number) => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  return (
    <Input
      id={id}
      type="number"
      min={8}
      max={32}
      value={draft ?? value}
      onChange={(e) => {
        setDraft(e.target.value);
        const n = Number(e.target.value);
        if (Number.isInteger(n) && n >= 8 && n <= 32) onChange(n);
      }}
      onBlur={() => setDraft(null)}
    />
  );
}

function Swatches({ scheme }: { scheme: TerminalScheme }) {
  const t = scheme.theme;
  return (
    <span className="flex overflow-hidden rounded-sm border" aria-hidden>
      {[t.background, t.red, t.green, t.yellow, t.blue, t.magenta].map((c, i) => (
        <span key={i} className="size-3" style={{ background: c }} />
      ))}
    </span>
  );
}

/** A fake session in the chosen scheme. Unset ANSI colours fall back to xterm's defaults. */
function Preview({
  scheme,
  family,
  size,
}: {
  scheme: TerminalScheme;
  family: string | null;
  size: number;
}) {
  const t = scheme.theme;
  const c = (name: (typeof ANSI)[number], bright = false) => {
    const key = (
      bright ? `bright${name[0].toUpperCase()}${name.slice(1)}` : name
    ) as keyof typeof t;
    return (
      (t[key] as string | undefined) ??
      XTERM_DEFAULTS[bright ? 8 + ANSI.indexOf(name) : ANSI.indexOf(name)]
    );
  };
  return (
    <div
      role="img"
      aria-label={`Preview of ${scheme.name}`}
      className="overflow-hidden rounded-md border p-3 leading-snug whitespace-pre"
      style={{
        background: t.background,
        color: t.foreground,
        fontFamily: fontStack(family),
        fontSize: size,
      }}
    >
      <div>
        <span style={{ color: c("green", true) }}>tom@devbox</span>:
        <span style={{ color: c("blue", true) }}>~/app</span>$ ls
      </div>
      <div>
        <span style={{ color: c("blue", true) }}>config</span>
        {"  "}
        <span style={{ color: c("green", true) }}>deploy.sh</span>
        {"  "}README.md{"  "}
        <span style={{ color: c("cyan", true) }}>current</span>
        {"  "}
        <span style={{ color: c("red", true) }}>backup.tar.gz</span>
      </div>
      <div>
        <span style={{ color: c("red") }}>ERROR</span> disk full{"  "}
        <span style={{ color: c("yellow") }}>WARN</span> slow{"  "}
        <span style={{ color: c("green") }}>OK</span> done{"  "}
        <span style={{ color: c("magenta") }}>branch</span>{" "}
        <span style={{ color: c("black", true) }}>dim comment</span>
      </div>
      <div className="mt-2 flex gap-1">
        {ANSI.flatMap((n) => [c(n), c(n, true)]).map((col, i) => (
          <span key={i} className="h-3 w-5 rounded-sm" style={{ background: col }} />
        ))}
      </div>
    </div>
  );
}

/** xterm.js's built-in 16 colours, used where a scheme leaves one unset ("Auto" in dark mode). */
const XTERM_DEFAULTS = [
  "#2e3436",
  "#cc0000",
  "#4e9a06",
  "#c4a000",
  "#3465a4",
  "#75507b",
  "#06989a",
  "#d3d7cf",
  "#555753",
  "#ef2929",
  "#8ae234",
  "#fce94f",
  "#729fcf",
  "#ad7fa8",
  "#34e2e2",
  "#eeeeec",
];
