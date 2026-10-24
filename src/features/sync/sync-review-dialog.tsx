import { useMemo } from "react";
import {
  ArrowRightIcon,
  CheckIcon,
  FileCodeIcon,
  FileInputIcon,
  GitCompareIcon,
  RefreshCwIcon,
  Trash2Icon,
  TriangleAlertIcon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { connectionLabel, formatTs } from "@/features/hosts/format";
import { useHostActions } from "@/features/hosts/host-actions";
import { useHostsData } from "@/features/hosts/queries";
import type { Host } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/ui";
import { compareWithConfig, reviewItems } from "./compare";
import {
  useAcceptRename,
  useAcknowledge,
  useKeepDbOnly,
  useLastSync,
  useRenameSuggestions,
  useResolveConflict,
  useRunSync,
} from "./queries";

/** Review of the last ~/.ssh/config scan. Only changes the database, never the config file. */
export function SyncReviewDialog() {
  const open = useUiStore((s) => s.syncReviewOpen);
  const setOpen = useUiStore((s) => s.setSyncReviewOpen);
  const hosts = useHostsData();
  const report = useLastSync().data;
  const renames = useRenameSuggestions().data ?? [];
  const rescan = useRunSync();
  const items = useMemo(() => reviewItems(hosts), [hosts]);
  const byId = useMemo(() => new Map(hosts.map((h) => [h.id, h])), [hosts]);
  const renamePairs = renames
    .map((r) => ({ from: byId.get(r.fromId), to: byId.get(r.toId) }))
    .filter((p): p is { from: Host; to: Host } => !!p.from && !!p.to);

  const tabs = [
    { id: "conflicts", label: "Conflicts", count: items.conflicts.length },
    { id: "renames", label: "Renamed?", count: renamePairs.length },
    { id: "new", label: "New", count: items.added.length },
    { id: "removed", label: "Removed", count: items.removed.length },
    { id: "rules", label: "Rules", count: report?.rules.length ?? 0 },
    { id: "warnings", label: "Warnings", count: report?.warnings.length ?? 0 },
  ];
  const firstWithItems = tabs.find((t) => t.count > 0 && t.id !== "rules")?.id ?? "new";

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="flex h-[80vh] flex-col gap-3 sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>SSH config review</DialogTitle>
          <DialogDescription>
            {report ? (
              <>
                Scanned <code>{report.configPath}</code> at {formatTs(report.scannedAt)}:{" "}
                {report.configHosts} hosts. Decisions here only change RidgeSSH&apos;s database —
                your SSH config file is not modified.
              </>
            ) : (
              "No scan yet in this session."
            )}
          </DialogDescription>
        </DialogHeader>

        <Tabs key={String(open)} defaultValue={firstWithItems} className="min-h-0 flex-1">
          <div className="flex items-center gap-2">
            <TabsList>
              {tabs.map((t) => (
                <TabsTrigger key={t.id} value={t.id}>
                  {t.label}
                  {t.count > 0 && (
                    <Badge variant="secondary" className="ml-1 h-4 px-1 tabular-nums">
                      {t.count}
                    </Badge>
                  )}
                </TabsTrigger>
              ))}
            </TabsList>
            <Button
              variant="outline"
              size="sm"
              className="ml-auto"
              disabled={rescan.isPending}
              onClick={() => rescan.mutate()}
            >
              <RefreshCwIcon className={cn(rescan.isPending && "animate-spin")} /> Rescan
            </Button>
          </div>

          <ScrollArea className="min-h-0 flex-1 rounded-md border">
            <TabsContent value="conflicts" className="p-3">
              <ConflictList hosts={items.conflicts} />
            </TabsContent>
            <TabsContent value="renames" className="p-3">
              <RenameList pairs={renamePairs} />
            </TabsContent>
            <TabsContent value="new" className="p-3">
              <NewList hosts={items.added} />
            </TabsContent>
            <TabsContent value="removed" className="p-3">
              <RemovedList hosts={items.removed} />
            </TabsContent>
            <TabsContent value="rules" className="p-3">
              <RuleList rules={report?.rules ?? []} />
            </TabsContent>
            <TabsContent value="warnings" className="p-3">
              {report?.warnings.length ? (
                <ul className="grid gap-2 text-sm">
                  {report.warnings.map((w, i) => (
                    <li key={i} className="flex gap-2">
                      <TriangleAlertIcon className="mt-0.5 size-4 shrink-0 text-amber-500" />
                      {w}
                    </li>
                  ))}
                </ul>
              ) : (
                <Nothing text="No warnings." />
              )}
            </TabsContent>
          </ScrollArea>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}

function Nothing({ text }: { text: string }) {
  return <p className="p-6 text-center text-sm text-muted-foreground">{text}</p>;
}

function HostLine({ host }: { host: Host }) {
  return (
    <div className="min-w-0">
      <div className="truncate font-medium">{host.alias}</div>
      <div className="truncate text-xs text-muted-foreground">
        {connectionLabel(host)}
        {host.configSource && ` · from ${host.configSource}`}
      </div>
    </div>
  );
}

function NewList({ hosts }: { hosts: Host[] }) {
  const ack = useAcknowledge();
  if (!hosts.length) return <Nothing text="No new hosts from the config." />;
  return (
    <div className="grid gap-2">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          Found in the config and added to RidgeSSH. Mark them reviewed once you have grouped or
          annotated them.
        </p>
        <Button size="sm" onClick={() => ack.mutate(hosts.map((h) => h.id))}>
          <CheckIcon /> Mark all reviewed
        </Button>
      </div>
      <ul className="divide-y rounded-md border">
        {hosts.map((h) => (
          <li key={h.id} className="flex items-center gap-3 px-3 py-2">
            <HostLine host={h} />
            <Button
              variant="ghost"
              size="sm"
              className="ml-auto"
              onClick={() => ack.mutate([h.id])}
            >
              <CheckIcon /> Reviewed
            </Button>
          </li>
        ))}
      </ul>
    </div>
  );
}

function RemovedList({ hosts }: { hosts: Host[] }) {
  const keep = useKeepDbOnly();
  const { trash } = useHostActions();
  // Hand over to the write dialog (one dialog at a time).
  const restoreIntoConfig = (ids: string[]) => {
    useUiStore.getState().setSyncReviewOpen(false);
    useUiStore.getState().openConfigWrite(ids);
  };
  if (!hosts.length) return <Nothing text="Nothing was removed from the config." />;
  return (
    <div className="grid gap-2">
      <div className="flex items-center justify-between gap-2">
        <p className="text-sm text-muted-foreground">
          These hosts are no longer in the config. Their notes and history are kept until you
          decide.
        </p>
        <div className="flex shrink-0 gap-1">
          <Button size="sm" variant="outline" onClick={() => keep.mutate(hosts.map((h) => h.id))}>
            Keep all
          </Button>
          <Button size="sm" variant="outline" onClick={() => trash(hosts)}>
            <Trash2Icon /> Trash all
          </Button>
        </div>
      </div>
      <ul className="divide-y rounded-md border">
        {hosts.map((h) => (
          <li key={h.id} className="flex items-center gap-3 px-3 py-2">
            <HostLine host={h} />
            <span className="ml-auto shrink-0 text-xs text-muted-foreground">
              removed {formatTs(h.removedFromConfigAt)}
            </span>
            <Button variant="ghost" size="sm" onClick={() => restoreIntoConfig([h.id])}>
              <FileInputIcon /> Restore into config
            </Button>
            <Button variant="ghost" size="sm" onClick={() => keep.mutate([h.id])}>
              Keep in RidgeSSH
            </Button>
            <Button variant="ghost" size="sm" onClick={() => trash([h])}>
              <Trash2Icon /> Trash
            </Button>
          </li>
        ))}
      </ul>
    </div>
  );
}

function ConflictList({ hosts }: { hosts: Host[] }) {
  const resolve = useResolveConflict();
  if (!hosts.length) return <Nothing text="No conflicts." />;
  return (
    <div className="grid gap-4">
      <p className="text-sm text-muted-foreground">
        These hosts were changed both in RidgeSSH and in the config file. Pick which version to
        keep. Keeping yours queues it for the config; the file is only written after you confirm it.
      </p>
      {hosts.map((h) => (
        <div key={h.id} className="rounded-md border">
          <div className="flex items-center gap-2 border-b px-3 py-2">
            <GitCompareIcon className="size-4 text-destructive" />
            <span className="font-medium">{h.alias}</span>
            <div className="ml-auto flex gap-1">
              <Button
                size="sm"
                variant="outline"
                onClick={() => resolve.mutate({ id: h.id, keep: "config" })}
              >
                Use config version
              </Button>
              <Button size="sm" onClick={() => resolve.mutate({ id: h.id, keep: "database" })}>
                Keep mine
              </Button>
            </div>
          </div>
          <table className="w-full text-sm">
            <thead className="text-xs text-muted-foreground">
              <tr>
                <th className="w-36 px-3 py-1.5 text-left font-medium">Field</th>
                <th className="px-3 py-1.5 text-left font-medium">RidgeSSH (yours)</th>
                <th className="px-3 py-1.5 text-left font-medium">SSH config</th>
              </tr>
            </thead>
            <tbody>
              {compareWithConfig(h).map((r) => (
                <tr key={r.label} className={cn("border-t", r.differs && "bg-amber-500/10")}>
                  <td className="px-3 py-1.5 text-muted-foreground">{r.label}</td>
                  <td className="px-3 py-1.5 font-mono text-xs whitespace-pre-wrap">{r.db}</td>
                  <td className="px-3 py-1.5 font-mono text-xs whitespace-pre-wrap">{r.config}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ))}
    </div>
  );
}

function RenameList({ pairs }: { pairs: { from: Host; to: Host }[] }) {
  const accept = useAcceptRename();
  const ack = useAcknowledge();
  if (!pairs.length) return <Nothing text="No renamed hosts detected." />;
  return (
    <div className="grid gap-2">
      <p className="text-sm text-muted-foreground">
        A host disappeared and a new one with the same target appeared. If it was renamed, merge
        them to keep the notes, group, tags and custom fields. The old entry goes to trash.
      </p>
      <ul className="divide-y rounded-md border">
        {pairs.map(({ from, to }) => (
          <li key={from.id} className="flex items-center gap-3 px-3 py-2">
            <HostLine host={from} />
            <ArrowRightIcon className="size-4 shrink-0 text-muted-foreground" />
            <HostLine host={to} />
            <div className="ml-auto flex shrink-0 gap-1">
              <Button size="sm" onClick={() => accept.mutate({ fromId: from.id, toId: to.id })}>
                Merge
              </Button>
              <Button size="sm" variant="ghost" onClick={() => ack.mutate([to.id])}>
                Not a rename
              </Button>
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}

function RuleList({
  rules,
}: {
  rules: { kind: string; header: string; source: string; line: number; directiveCount: number }[];
}) {
  if (!rules.length) return <Nothing text="No wildcard or Match blocks." />;
  return (
    <div className="grid gap-2">
      <p className="text-sm text-muted-foreground">
        Wildcard <code>Host</code> patterns and <code>Match</code> blocks apply to many hosts.
        RidgeSSH shows them but never edits them.
      </p>
      <ul className="divide-y rounded-md border">
        {rules.map((r, i) => (
          <li key={i} className="flex items-center gap-3 px-3 py-2 text-sm">
            <FileCodeIcon className="size-4 shrink-0 text-muted-foreground" />
            <code className="min-w-0 truncate">
              {r.kind === "host" ? "Host" : "Match"} {r.header}
            </code>
            <span className="ml-auto shrink-0 text-xs text-muted-foreground">
              {r.directiveCount} option{r.directiveCount === 1 ? "" : "s"} · line {r.line}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
