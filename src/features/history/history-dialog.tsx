import { useDeferredValue, useMemo, useState } from "react";
import { SearchIcon, Undo2Icon } from "lucide-react";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { formatTs } from "@/features/hosts/format";
import type { AuditEntry, HistoryFilter } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { HistoryEntry, HistoryEntryIcon } from "./history-entry";
import { canRevert, SOURCE_LABELS, useHistory, useRevertBatch } from "./queries";

const TYPES = [
  ["all", "Everything"],
  ["host", "Hosts"],
  ["group", "Groups"],
  ["settings", "Settings"],
  ["backup", "Backups"],
] as const;

const SOURCES = [
  ["all", "Any source"],
  ["user", "Your changes"],
  ["sync", "Config scans"],
  ["restore", "Undo / restore"],
  ["import", "Imports"],
  ["system", "Automatic"],
] as const;

interface Batch {
  id: string;
  entries: AuditEntry[];
}

/** Consecutive entries of one action (transaction) are shown together. */
function groupBatches(entries: AuditEntry[]): Batch[] {
  const out: Batch[] = [];
  for (const e of entries) {
    const key = e.batchId ?? `entry-${e.id}`;
    const last = out[out.length - 1];
    if (last && last.id === key) last.entries.push(e);
    else out.push({ id: key, entries: [e] });
  }
  return out;
}

export function HistoryDialog() {
  const open = useUiStore((s) => s.historyOpen);
  const setOpen = useUiStore((s) => s.setHistoryOpen);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="flex h-[85vh] flex-col gap-3 sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>History</DialogTitle>
          <DialogDescription>
            Every change to hosts, groups and settings. Undo reverts what one change did and keeps
            later edits; connection changes then show up as pending for the SSH config.
          </DialogDescription>
        </DialogHeader>
        {open && <Body />}
      </DialogContent>
    </Dialog>
  );
}

function Body() {
  const [type, setType] = useState<(typeof TYPES)[number][0]>("all");
  const [source, setSource] = useState<(typeof SOURCES)[number][0]>("all");
  const [search, setSearch] = useState("");
  const deferred = useDeferredValue(search.trim());
  const filter: HistoryFilter = {
    entityType: type === "all" ? null : type,
    source: source === "all" ? null : source,
    search: deferred || null,
  };
  const history = useHistory(filter);
  const batches = useMemo(() => groupBatches(history.data?.pages.flat() ?? []), [history.data]);
  const undoAll = useRevertBatch();
  const [confirming, setConfirming] = useState<Batch | null>(null);

  return (
    <>
      <div className="flex flex-wrap gap-2">
        <div className="relative min-w-48 flex-1">
          <SearchIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search alias, hostname, notes…"
            aria-label="Search history"
            className="h-8 pl-8"
          />
        </div>
        <Select value={type} onValueChange={(v) => setType(v as typeof type)}>
          <SelectTrigger size="sm" className="w-36" aria-label="Kind">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {TYPES.map(([v, l]) => (
              <SelectItem key={v} value={v}>
                {l}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select value={source} onValueChange={(v) => setSource(v as typeof source)}>
          <SelectTrigger size="sm" className="w-40" aria-label="Source">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SOURCES.map(([v, l]) => (
              <SelectItem key={v} value={v}>
                {l}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <ScrollArea className="min-h-0 flex-1 rounded-md border">
        {batches.length === 0 ? (
          <p className="p-6 text-center text-sm text-muted-foreground">
            {history.isPending ? "Loading…" : "No matching changes."}
          </p>
        ) : (
          <ol className="divide-y" aria-label="Changes">
            {batches.map((b) => {
              const first = b.entries[0];
              const multi = b.entries.length > 1;
              return (
                <li key={b.id} className="grid gap-3 px-3 py-3">
                  {multi && (
                    <div className="flex items-center gap-2 text-xs text-muted-foreground">
                      <span>{formatTs(first.ts)}</span>
                      <Badge variant="secondary">{b.entries.length} changes</Badge>
                      <span>by {SOURCE_LABELS[first.source]}</span>
                      {first.batchId && b.entries.some(canRevert) && (
                        <Button
                          size="xs"
                          variant="outline"
                          className="ml-auto"
                          onClick={() => setConfirming(b)}
                        >
                          <Undo2Icon /> Undo all
                        </Button>
                      )}
                    </div>
                  )}
                  {b.entries.map((e) => (
                    <div key={e.id} className="flex gap-3">
                      <HistoryEntryIcon entry={e} />
                      <HistoryEntry entry={e} showLabel />
                    </div>
                  ))}
                </li>
              );
            })}
          </ol>
        )}
        {history.hasNextPage && (
          <div className="p-3 text-center">
            <Button
              variant="outline"
              size="sm"
              disabled={history.isFetchingNextPage}
              onClick={() => void history.fetchNextPage()}
            >
              Load older changes
            </Button>
          </div>
        )}
      </ScrollArea>

      <ConfirmDialog
        open={confirming !== null}
        onOpenChange={(o) => !o && setConfirming(null)}
        title={`Undo ${confirming?.entries.length} changes?`}
        description="Everything this action changed is reverted, newest first. The undo itself is recorded and can be undone again."
        confirmLabel="Undo all"
        onConfirm={() => {
          if (confirming?.entries[0].batchId) undoAll.mutate(confirming.entries[0].batchId);
          setConfirming(null);
        }}
      />
    </>
  );
}
