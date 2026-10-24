import type { ImportStrategy, ItemStatus } from "@/lib/ipc";

export const STATUS_LABELS: Record<ItemStatus, string> = {
  new: "New",
  same: "Already here",
  differs: "Differs",
};

export const STRATEGY_LABELS: Record<ImportStrategy, string> = {
  import: "Import",
  skip: "Skip",
  overwrite: "Overwrite",
  copy: "Import as copy",
};

/** The choices an item offers (mirrors `ItemStatus::allows` in Rust). */
export const strategiesFor = (status: ItemStatus): ImportStrategy[] =>
  status === "new" ? ["import", "skip"] : ["skip", "overwrite", "copy"];

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** "3 hosts, 1 script will be imported" for the dialog footer. */
export function summarize(
  hosts: Record<string, ImportStrategy>,
  scripts: Record<string, ImportStrategy>,
): { count: number; text: string } {
  const h = Object.values(hosts).filter((s) => s !== "skip").length;
  const s = Object.values(scripts).filter((v) => v !== "skip").length;
  const parts = [h && plural(h, "host"), s && plural(s, "script")].filter(Boolean);
  return {
    count: h + s,
    text: parts.length ? `${parts.join(", ")} will be imported` : "Nothing selected",
  };
}
