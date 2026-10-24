import type { DiffLine } from "@/lib/ipc";
import { cn } from "@/lib/utils";

const ROW: Record<DiffLine["kind"], string> = {
  hunk: "bg-muted text-muted-foreground",
  context: "",
  add: "bg-emerald-500/10 text-emerald-800 dark:text-emerald-300",
  remove: "bg-red-500/10 text-red-800 dark:text-red-300",
};
const SIGN: Record<DiffLine["kind"], string> = { hunk: "", context: " ", add: "+", remove: "-" };

/** Unified diff with old/new line numbers. */
export function DiffView({ lines }: { lines: DiffLine[] }) {
  return (
    <table className="w-full border-collapse font-mono text-xs" aria-label="Config diff">
      <tbody>
        {lines.map((l, i) => (
          <tr key={i} data-kind={l.kind} className={ROW[l.kind]}>
            {l.kind === "hunk" ? (
              <td colSpan={4} className="px-3 py-1">
                {l.text}
              </td>
            ) : (
              <>
                <td className="w-10 pr-2 text-right text-muted-foreground/70 select-none">
                  {l.old}
                </td>
                <td className="w-10 pr-2 text-right text-muted-foreground/70 select-none">
                  {l.new}
                </td>
                <td className="w-4 select-none">{SIGN[l.kind]}</td>
                <td className={cn("pr-3 whitespace-pre", !l.text && "h-4")}>{l.text}</td>
              </>
            )}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
