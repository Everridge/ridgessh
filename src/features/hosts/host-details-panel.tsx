import {
  ArrowUpDownIcon,
  ExternalLinkIcon,
  FileCodeIcon,
  MousePointerClickIcon,
  PencilIcon,
  PlayIcon,
  RotateCcwIcon,
  SquareTerminalIcon,
  StarIcon,
  Trash2Icon,
} from "lucide-react";
import { HostSessions } from "@/features/terminal/host-sessions";
import { useConnect } from "@/features/terminal/use-connect";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { HostHistory } from "@/features/history/host-history";
import type { Host } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { offeredOp, opAction } from "@/features/config-write/pending";
import { useUiStore } from "@/stores/ui";
import { groupPath } from "./filter";
import { connectionLabel, formatTs } from "./format";
import { useHostActions, usePurgeConfirm } from "./host-actions";
import { SyncBadges } from "./host-badges";
import { useGroupsData, useHostsData, useSetFavorite } from "./queries";

const NEEDS_REVIEW = new Set(["conflict", "config_only_new", "removed_from_config"]);

export function HostDetailsPanel() {
  const selectedId = useUiStore((s) => s.selectedHostId);
  const host = useHostsData().find((h) => h.id === selectedId);

  if (!host) {
    return (
      <section className="flex h-full flex-col">
        <div className="flex h-10 items-center border-b px-3">
          <h2 className="text-sm font-medium">Details</h2>
        </div>
        <div className="flex flex-1 flex-col items-center justify-center gap-2 p-6 text-center text-sm text-muted-foreground">
          <MousePointerClickIcon className="size-8" />
          Select a host to see its details.
        </div>
      </section>
    );
  }
  return <Details key={host.id} host={host} />;
}

function Details({ host }: { host: Host }) {
  const groups = useGroupsData();
  const openHostForm = useUiStore((s) => s.openHostForm);
  const openReview = useUiStore((s) => s.setSyncReviewOpen);
  const fav = useSetFavorite();
  const actions = useHostActions();
  const purge = usePurgeConfirm();
  const { connect, connectExternal } = useConnect();
  const trashed = host.deletedAt !== null;

  return (
    <section className="flex h-full min-w-0 flex-col">
      <div className="flex h-10 shrink-0 items-center gap-1 border-b px-3">
        <h2 className="truncate text-sm font-medium">{host.alias}</h2>
        <div className="ml-auto flex items-center">
          {trashed ? (
            <>
              <Button variant="ghost" size="sm" onClick={() => actions.restore(host)}>
                <RotateCcwIcon /> Restore
              </Button>
              <Button
                variant="ghost"
                size="sm"
                className="text-destructive"
                onClick={() => purge.ask(host)}
              >
                <Trash2Icon /> Delete
              </Button>
            </>
          ) : (
            <>
              <Button size="sm" className="mr-1" onClick={() => connect(host)}>
                <SquareTerminalIcon /> Connect
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Open in external terminal"
                title="Open in external terminal"
                onClick={() => connectExternal(host)}
              >
                <ExternalLinkIcon />
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Copy files"
                title="Copy files (scp)"
                onClick={() => useUiStore.getState().openTransfer(host.id, "upload")}
              >
                <ArrowUpDownIcon />
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Run script"
                title="Run script"
                onClick={() => useUiStore.getState().openScriptRun([host.id])}
              >
                <PlayIcon />
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={host.favorite ? "Remove from favorites" : "Add to favorites"}
                onClick={() => fav.mutate({ id: host.id, favorite: !host.favorite })}
              >
                <StarIcon className={cn(host.favorite && "fill-amber-400 text-amber-500")} />
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Edit"
                onClick={() => openHostForm(host.id)}
              >
                <PencilIcon />
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Delete"
                onClick={() => actions.trash([host])}
              >
                <Trash2Icon />
              </Button>
            </>
          )}
        </div>
      </div>

      <Tabs defaultValue="details" className="min-h-0 flex-1 gap-0">
        <TabsList variant="line" className="w-full justify-start border-b px-3">
          <TabsTrigger value="details">Details</TabsTrigger>
          <TabsTrigger value="sessions">Sessions</TabsTrigger>
          <TabsTrigger value="history">History</TabsTrigger>
        </TabsList>
        <TabsContent value="sessions" className="min-h-0">
          <ScrollArea className="h-full">
            <HostSessions hostId={host.id} />
          </ScrollArea>
        </TabsContent>
        <TabsContent value="details" className="min-h-0">
          <ScrollArea className="h-full">
            <div className="grid gap-5 p-4">
              <div className="grid gap-1.5">
                <div className="flex flex-wrap items-center gap-2">
                  <code className="text-sm">{connectionLabel(host)}</code>
                  <SyncBadges host={host} />
                  {NEEDS_REVIEW.has(host.syncState) && (
                    <Button
                      variant="link"
                      size="xs"
                      className="h-auto p-0"
                      onClick={() => openReview(true)}
                    >
                      Review
                    </Button>
                  )}
                </div>
                {host.description && <p className="text-sm">{host.description}</p>}
                <ConfigCallout host={host} />
                {host.tags.length > 0 && (
                  <div className="flex flex-wrap gap-1">
                    {host.tags.map((t) => (
                      <Badge key={t.id} variant="secondary">
                        {t.name}
                      </Badge>
                    ))}
                  </div>
                )}
              </div>

              <Section title="Connection">
                <Row label="Host">{host.alias}</Row>
                <Row label="Hostname">{host.hostname}</Row>
                <Row label="User">{host.user}</Row>
                <Row label="Port">{host.port}</Row>
                <Row label="Identity file">{host.identityFile}</Row>
                <Row label="ProxyJump">{host.proxyJump}</Row>
                {host.extraOptions.map((o, i) => (
                  <Row key={i} label={o.key}>
                    {o.value}
                  </Row>
                ))}
              </Section>

              {host.fields.length > 0 && (
                <Section title="Custom fields">
                  {host.fields.map((f, i) => (
                    <Row key={i} label={f.key} mono={false}>
                      {f.value}
                    </Row>
                  ))}
                </Section>
              )}

              {host.notes && (
                <Section title="Notes">
                  <p className="text-sm whitespace-pre-wrap">{host.notes}</p>
                </Section>
              )}

              <Section title="Info">
                <Row label="Group" mono={false}>
                  {groupPath(groups, host.groupId) || null}
                </Row>
                <Row label="SSH config" mono={false}>
                  {host.configSnapshot
                    ? host.configSource
                      ? `In ${host.configSource}`
                      : "In ~/.ssh/config"
                    : "Not in the config"}
                </Row>
                {host.lastSeenInConfigAt && (
                  <Row label="Last seen in config" mono={false}>
                    {formatTs(host.lastSeenInConfigAt)}
                  </Row>
                )}
                <Row label="Created" mono={false}>
                  {formatTs(host.createdAt)}
                </Row>
                <Row label="Updated" mono={false}>
                  {formatTs(host.updatedAt)}
                </Row>
                <Row label="Last connected" mono={false}>
                  {host.lastConnectedAt && formatTs(host.lastConnectedAt)}
                </Row>
                {trashed && (
                  <Row label="Deleted" mono={false}>
                    {formatTs(host.deletedAt)}
                  </Row>
                )}
              </Section>
            </div>
          </ScrollArea>
        </TabsContent>
        <TabsContent value="history" className="min-h-0">
          <ScrollArea className="h-full">
            <HostHistory hostId={host.id} />
          </ScrollArea>
        </TabsContent>
      </Tabs>
      {purge.dialog}
    </section>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="grid gap-1.5">
      <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">{title}</h3>
      <dl className="grid gap-1">{children}</dl>
    </div>
  );
}

function Row({
  label,
  children,
  mono = true,
}: {
  label: string;
  children: React.ReactNode;
  mono?: boolean;
}) {
  const empty = children === null || children === undefined || children === "";
  return (
    <div className="grid grid-cols-[8rem_1fr] gap-2 text-sm">
      <dt className="truncate text-muted-foreground">{label}</dt>
      <dd
        className={cn(
          "min-w-0 break-words",
          mono && !empty && "font-mono text-[13px]",
          empty && "text-muted-foreground",
        )}
      >
        {empty ? "—" : children}
      </dd>
    </div>
  );
}

const CALLOUT: Record<string, string> = {
  update: "Connection changes are not in ~/.ssh/config yet.",
  add: "Only in RidgeSSH, not in ~/.ssh/config.",
  restore: "No longer in ~/.ssh/config. Its settings are kept here.",
  remove: "In trash, but still in ~/.ssh/config.",
};

/** What this host could write to the config, with a shortcut into the write dialog. */
function ConfigCallout({ host }: { host: Host }) {
  const openWrite = useUiStore((s) => s.openConfigWrite);
  const op = offeredOp(host);
  if (!op || host.syncState === "conflict") return null;
  const key = op === "add" && host.syncState === "removed_from_config" ? "restore" : op;
  return (
    <div
      className={cn(
        "mt-1 flex items-center gap-2 rounded-md border px-3 py-1.5 text-sm",
        op === "update" ? "border-violet-500/40 bg-violet-500/5" : "border-dashed",
      )}
    >
      <FileCodeIcon className="size-4 shrink-0 text-muted-foreground" />
      <span className="min-w-0 flex-1 text-muted-foreground">{CALLOUT[key]}</span>
      <Button size="xs" variant="outline" onClick={() => openWrite([host.id])}>
        {opAction(op, host)}
      </Button>
    </div>
  );
}
