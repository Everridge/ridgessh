import { useCallback, useMemo, useState } from "react";
import { ConfirmDialog } from "@/components/confirm-dialog";
import {
  ChevronDownIcon,
  ChevronRightIcon,
  CornerDownRightIcon,
  DownloadIcon,
  KeyRoundIcon,
  LayersIcon,
  ExternalLinkIcon,
  FileDownIcon,
  FolderDownIcon,
  FolderInputIcon,
  PencilIcon,
  PlayIcon,
  PlusIcon,
  RotateCcwIcon,
  SearchXIcon,
  ServerIcon,
  SquareTerminalIcon,
  StarIcon,
  Trash2Icon,
  UploadIcon,
  XIcon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { Group, Host, UserVariant } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { useUiStore, type HostView } from "@/stores/ui";
import {
  buildGroupTree,
  filterHosts,
  flattenTree,
  groupByServer,
  groupPath,
  type ListEntry,
} from "./filter";
import { connectionLabel } from "./format";
import { useHostActions, usePurgeConfirm } from "./host-actions";
import { SyncBadges } from "./host-badges";
import {
  configAlias,
  useEmptyTrash,
  useGroupsData,
  useHosts,
  useMoveHosts,
  useSetFavorite,
  useVariants,
  variantsFor,
} from "./queries";
import { useConnect } from "@/features/terminal/use-connect";
import { stillInConfig } from "@/features/config-write/pending";

const VIEW_TITLES: Record<Exclude<HostView, "group">, string> = {
  all: "All hosts",
  favorites: "Favorites",
  recent: "Recent",
  removed: "Removed from config",
  trash: "Trash",
};

export function HostListPanel() {
  const hostsQuery = useHosts();
  const groups = useGroupsData();
  const { view, groupId, search, checkedIds, setChecked, openHostForm, groupServers } =
    useUiStore();
  const actions = useHostActions();
  const move = useMoveHosts();
  const variants = useVariants();
  // Searching for "tom" also finds hosts that have a Match block for user tom.
  const extraText = useCallback(
    (h: Host) =>
      variantsFor(h, variants)
        .map((v) => ` ${v.user}@${configAlias(h)} ${v.identityFile ?? ""}`)
        .join(""),
    [variants],
  );

  const hosts = useMemo(
    () => filterHosts(hostsQuery.data ?? [], groups, { view, groupId, search, extraText }),
    [hostsQuery.data, groups, view, groupId, search, extraText],
  );
  // Recent keeps its time order and trash stays flat; elsewhere same-server hosts nest.
  const entries: ListEntry[] = useMemo(
    () =>
      groupServers && view !== "recent" && view !== "trash"
        ? groupByServer(hosts)
        : hosts.map((host) => ({ kind: "host", host })),
    [hosts, groupServers, view],
  );
  const checked = hosts.filter((h) => checkedIds.includes(h.id));
  const allChecked = hosts.length > 0 && checked.length === hosts.length;
  const title = view === "group" ? groupPath(groups, groupId) : VIEW_TITLES[view];

  return (
    <section className="flex h-full flex-col">
      <div className="flex h-10 shrink-0 items-center gap-2 border-b px-3">
        <Checkbox
          aria-label="Select all"
          checked={allChecked ? true : checked.length ? "indeterminate" : false}
          onCheckedChange={() => setChecked(allChecked ? [] : hosts.map((h) => h.id))}
          disabled={!hosts.length}
        />
        {checked.length > 0 ? (
          <>
            <span className="text-sm font-medium">{checked.length} selected</span>
            <div className="ml-auto flex items-center gap-1">
              {view !== "trash" && (
                <>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => useUiStore.getState().openScriptRun(checked.map((h) => h.id))}
                  >
                    <PlayIcon /> Run script
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => useUiStore.getState().openExport(checked.map((h) => h.id))}
                  >
                    <FileDownIcon /> Export
                  </Button>
                  <MoveMenu
                    groups={groups}
                    onMove={(g) => move.mutate({ ids: checked.map((h) => h.id), groupId: g })}
                  />
                  <Button variant="ghost" size="sm" onClick={() => actions.trash(checked)}>
                    <Trash2Icon /> Delete
                  </Button>
                </>
              )}
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Clear selection"
                onClick={() => setChecked([])}
              >
                <XIcon />
              </Button>
            </div>
          </>
        ) : (
          <>
            <h2 className="truncate text-sm font-medium">{title}</h2>
            <Badge variant="secondary" className="tabular-nums">
              {hosts.length}
            </Badge>
            {view !== "trash" ? (
              <>
                <Button
                  size="icon-sm"
                  variant={groupServers ? "secondary" : "ghost"}
                  className="ml-auto"
                  aria-pressed={groupServers}
                  aria-label="Group hosts by server"
                  title="Group hosts that point at the same server"
                  onClick={() => useUiStore.getState().setGroupServers(!groupServers)}
                >
                  <LayersIcon />
                </Button>
                <Button size="sm" onClick={() => openHostForm("new")}>
                  <PlusIcon /> New host
                </Button>
              </>
            ) : (
              hosts.length > 0 && <EmptyTrashButton hosts={hosts} />
            )}
          </>
        )}
      </div>

      {hostsQuery.isLoading ? null : hosts.length === 0 ? (
        <EmptyState view={view} searching={search.trim() !== ""} />
      ) : (
        <ScrollArea className="min-h-0 flex-1">
          <ul className="divide-y" aria-label="Hosts">
            {entries.map((e) =>
              e.kind === "host" ? (
                <HostRow
                  key={e.host.id}
                  host={e.host}
                  groups={groups}
                  variants={e.host.deletedAt ? [] : variantsFor(e.host, variants)}
                />
              ) : (
                <ServerRow key={e.key} entry={e} groups={groups} variants={variants} />
              ),
            )}
          </ul>
        </ScrollArea>
      )}
    </section>
  );
}

/** Several hosts on one server (same hostname and port), e.g. one alias per user. */
function ServerRow({
  entry,
  groups,
  variants,
}: {
  entry: Extract<ListEntry, { kind: "server" }>;
  groups: Group[];
  variants: UserVariant[];
}) {
  const [expanded, setExpanded] = useState(true);
  const checkedIds = useUiStore((s) => s.checkedIds);
  const ids = entry.hosts.map((h) => h.id);
  const on = ids.filter((id) => checkedIds.includes(id)).length;
  const users = [...new Set(entry.hosts.map((h) => h.user).filter(Boolean))].join(" · ");
  const toggleAll = () =>
    useUiStore
      .getState()
      .setChecked(
        on === ids.length
          ? checkedIds.filter((id) => !ids.includes(id))
          : [...new Set([...checkedIds, ...ids])],
      );
  const toggle = () => setExpanded((x) => !x);

  return (
    <li>
      <div
        role="button"
        tabIndex={0}
        aria-expanded={expanded}
        aria-label={`Server ${entry.name}, ${entry.hosts.length} hosts`}
        onClick={toggle}
        onKeyDown={(e) => e.key === "Enter" && toggle()}
        className="flex cursor-default items-center gap-3 bg-muted/20 px-3 py-1.5 outline-none hover:bg-muted/50 focus-visible:bg-muted/50"
      >
        <Checkbox
          aria-label={`Select all hosts on ${entry.name}`}
          checked={on === 0 ? false : on === ids.length ? true : "indeterminate"}
          onClick={(e) => e.stopPropagation()}
          onCheckedChange={toggleAll}
        />
        {expanded ? (
          <ChevronDownIcon className="size-4 shrink-0 text-muted-foreground" />
        ) : (
          <ChevronRightIcon className="size-4 shrink-0 text-muted-foreground" />
        )}
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <ServerIcon className="size-3.5 shrink-0 text-muted-foreground" />
            <span className="truncate font-medium">{entry.name}</span>
            <Badge variant="secondary" className="tabular-nums">
              {entry.hosts.length} hosts
            </Badge>
          </div>
          <div className="truncate text-xs text-muted-foreground">
            {entry.address}
            {users && ` · ${users}`}
          </div>
        </div>
      </div>
      {expanded && (
        <ul aria-label={`Hosts on ${entry.name}`} className="divide-y border-t">
          {entry.hosts.map((h) => (
            <HostRow
              key={h.id}
              host={h}
              groups={groups}
              variants={variantsFor(h, variants)}
              nested
            />
          ))}
        </ul>
      )}
    </li>
  );
}

function HostRow({
  host,
  groups,
  variants,
  nested = false,
}: {
  host: Host;
  groups: Group[];
  variants: UserVariant[];
  /** Shown inside a server entry. */
  nested?: boolean;
}) {
  const [expanded, setExpanded] = useState(true);
  const selected = useUiStore((s) => s.selectedHostId === host.id);
  const checked = useUiStore((s) => s.checkedIds.includes(host.id));
  const { selectHost, toggleChecked, openHostForm } = useUiStore.getState();
  const fav = useSetFavorite();
  const move = useMoveHosts();
  const actions = useHostActions();
  const purge = usePurgeConfirm();
  const { connect, connectExternal } = useConnect();
  const trashed = host.deletedAt !== null;
  const path = groupPath(groups, host.groupId);

  return (
    <li>
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <div
            role="button"
            tabIndex={0}
            onClick={() => selectHost(host.id)}
            onDoubleClick={() => !trashed && connect(host)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                selectHost(host.id);
                if (!trashed) connect(host);
              }
              if (e.key === "Delete" && !trashed) actions.trash([host]);
            }}
            className={cn(
              "flex cursor-default items-center gap-3 px-3 py-2 outline-none hover:bg-muted/50 focus-visible:bg-muted/50",
              nested && "pl-10",
              selected && "bg-muted",
            )}
          >
            <Checkbox
              aria-label={`Select ${host.alias}`}
              checked={checked}
              onClick={(e) => e.stopPropagation()}
              onCheckedChange={() => toggleChecked(host.id)}
            />
            <button
              type="button"
              aria-label={host.favorite ? "Remove from favorites" : "Add to favorites"}
              disabled={trashed}
              onClick={(e) => {
                e.stopPropagation();
                fav.mutate({ id: host.id, favorite: !host.favorite });
              }}
              className="text-muted-foreground hover:text-amber-500 disabled:opacity-40"
            >
              <StarIcon
                className={cn("size-4", host.favorite && "fill-amber-400 text-amber-500")}
              />
            </button>
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="truncate font-medium">{host.alias}</span>
                <SyncBadges host={host} />
                {variants.length > 0 && (
                  <button
                    type="button"
                    aria-expanded={expanded}
                    aria-label={`${expanded ? "Hide" : "Show"} users of ${host.alias}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      setExpanded((x) => !x);
                    }}
                    className="flex shrink-0 items-center gap-0.5 rounded px-1 text-xs text-muted-foreground hover:bg-muted"
                  >
                    {expanded ? (
                      <ChevronDownIcon className="size-3" />
                    ) : (
                      <ChevronRightIcon className="size-3" />
                    )}
                    {variants.length} user{variants.length === 1 ? "" : "s"}
                  </button>
                )}
              </div>
              <div className="truncate text-xs text-muted-foreground">
                {connectionLabel(host)}
                {host.description && ` · ${host.description}`}
              </div>
            </div>
            <div className="hidden max-w-[40%] shrink-0 items-center justify-end gap-1 overflow-hidden md:flex">
              {path && <span className="truncate text-xs text-muted-foreground">{path}</span>}
              {host.tags.slice(0, 3).map((t) => (
                <Badge key={t.id} variant="secondary">
                  {t.name}
                </Badge>
              ))}
              {host.tags.length > 3 && <Badge variant="secondary">+{host.tags.length - 3}</Badge>}
            </div>
          </div>
        </ContextMenuTrigger>
        <ContextMenuContent>
          {trashed ? (
            <>
              <ContextMenuItem onSelect={() => actions.restore(host)}>
                <RotateCcwIcon /> Restore
              </ContextMenuItem>
              <ContextMenuItem variant="destructive" onSelect={() => purge.ask(host)}>
                <Trash2Icon /> Delete permanently
              </ContextMenuItem>
            </>
          ) : (
            <>
              <ContextMenuItem onSelect={() => connect(host)}>
                <SquareTerminalIcon /> Connect
              </ContextMenuItem>
              <ContextMenuItem onSelect={() => connectExternal(host)}>
                <ExternalLinkIcon /> Open in external terminal
              </ContextMenuItem>
              <ContextMenuItem
                onSelect={() => useUiStore.getState().openTransfer(host.id, "upload")}
              >
                <UploadIcon /> Upload files…
              </ContextMenuItem>
              <ContextMenuItem
                onSelect={() => useUiStore.getState().openTransfer(host.id, "download")}
              >
                <DownloadIcon /> Download…
              </ContextMenuItem>
              <ContextMenuItem onSelect={() => useUiStore.getState().openFolderTransfer(host.id)}>
                <FolderDownIcon /> Download folder as zip…
              </ContextMenuItem>
              <ContextMenuItem
                onSelect={() => {
                  const { checkedIds, openScriptRun } = useUiStore.getState();
                  openScriptRun(checkedIds.includes(host.id) ? checkedIds : [host.id]);
                }}
              >
                <PlayIcon /> Run script…
              </ContextMenuItem>
              <ContextMenuItem
                onSelect={() => {
                  const { checkedIds, openExport } = useUiStore.getState();
                  openExport(checkedIds.includes(host.id) ? checkedIds : [host.id]);
                }}
              >
                <FileDownIcon /> Export…
              </ContextMenuItem>
              <ContextMenuSeparator />
              <ContextMenuItem onSelect={() => openHostForm(host.id)}>
                <PencilIcon /> Edit
              </ContextMenuItem>
              <ContextMenuItem
                onSelect={() => fav.mutate({ id: host.id, favorite: !host.favorite })}
              >
                <StarIcon /> {host.favorite ? "Unfavorite" : "Favorite"}
              </ContextMenuItem>
              <ContextMenuSub>
                <ContextMenuSubTrigger>
                  <FolderInputIcon /> Move to group
                </ContextMenuSubTrigger>
                <ContextMenuSubContent>
                  <ContextMenuItem onSelect={() => move.mutate({ ids: [host.id], groupId: null })}>
                    No group
                  </ContextMenuItem>
                  {flattenTree(buildGroupTree(groups)).map((g) => (
                    <ContextMenuItem
                      key={g.id}
                      onSelect={() => move.mutate({ ids: [host.id], groupId: g.id })}
                    >
                      <span style={{ paddingLeft: g.depth * 12 }}>{g.name}</span>
                    </ContextMenuItem>
                  ))}
                </ContextMenuSubContent>
              </ContextMenuSub>
              <ContextMenuSeparator />
              <ContextMenuItem variant="destructive" onSelect={() => actions.trash([host])}>
                <Trash2Icon /> Delete
              </ContextMenuItem>
            </>
          )}
        </ContextMenuContent>
      </ContextMenu>
      {expanded && variants.length > 0 && (
        <ul aria-label={`Users of ${host.alias}`}>
          {variants.map((v) => (
            <VariantRow key={v.user} host={host} variant={v} nested={nested} />
          ))}
        </ul>
      )}
      {purge.dialog}
    </li>
  );
}

const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;

/** One per-user `Match originalhost … user …` block, connected to as `user@alias`. */
function VariantRow({
  host,
  variant,
  nested,
}: {
  host: Host;
  variant: UserVariant;
  nested: boolean;
}) {
  const selectHost = useUiStore((s) => s.selectHost);
  const { connect, connectExternal } = useConnect();
  const { user } = variant;
  const target = `${user}@${configAlias(host)}`;
  return (
    <li>
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <div
            role="button"
            tabIndex={0}
            title={`ssh ${target} (Match block at line ${variant.line})`}
            onClick={() => selectHost(host.id)}
            onDoubleClick={() => connect(host, user)}
            onKeyDown={(e) => {
              if (e.key === "Enter") connect(host, user);
            }}
            className={cn(
              "flex cursor-default items-center gap-2 py-1 pr-3 text-sm outline-none hover:bg-muted/50 focus-visible:bg-muted/50",
              nested ? "pl-[5.5rem]" : "pl-[4.25rem]",
            )}
          >
            <CornerDownRightIcon className="size-3.5 shrink-0 text-muted-foreground" />
            <span className="truncate font-medium">{target}</span>
            {variant.identityFile && (
              <span className="flex min-w-0 items-center gap-1 text-xs text-muted-foreground">
                <KeyRoundIcon className="size-3 shrink-0" />
                <span className="truncate font-mono">{fileName(variant.identityFile)}</span>
              </span>
            )}
          </div>
        </ContextMenuTrigger>
        <ContextMenuContent>
          <ContextMenuItem onSelect={() => connect(host, user)}>
            <SquareTerminalIcon /> Connect as {user}
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => connectExternal(host, user)}>
            <ExternalLinkIcon /> Open in external terminal
          </ContextMenuItem>
          <ContextMenuItem
            onSelect={() => useUiStore.getState().openTransfer(host.id, "upload", user)}
          >
            <UploadIcon /> Upload files…
          </ContextMenuItem>
          <ContextMenuItem
            onSelect={() => useUiStore.getState().openTransfer(host.id, "download", user)}
          >
            <DownloadIcon /> Download…
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => useUiStore.getState().openFolderTransfer(host.id, user)}>
            <FolderDownIcon /> Download folder as zip…
          </ContextMenuItem>
          <ContextMenuItem
            onSelect={() => useUiStore.getState().openScriptRun([host.id], undefined, user)}
          >
            <PlayIcon /> Run script…
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
    </li>
  );
}

function MoveMenu({
  groups,
  onMove,
}: {
  groups: Group[];
  onMove: (groupId: string | null) => void;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm">
          <FolderInputIcon /> Move
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem onSelect={() => onMove(null)}>No group</DropdownMenuItem>
        {groups.length > 0 && <DropdownMenuSeparator />}
        {flattenTree(buildGroupTree(groups)).map((g) => (
          <DropdownMenuItem key={g.id} onSelect={() => onMove(g.id)}>
            <span style={{ paddingLeft: g.depth * 12 }}>{g.name}</span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function EmptyState({ view, searching }: { view: HostView; searching: boolean }) {
  const openHostForm = useUiStore((s) => s.openHostForm);
  if (searching) {
    return <Empty icon={SearchXIcon} title="No matches" text="No hosts match your search." />;
  }
  const text: Record<HostView, string> = {
    all: "Add a host, or add Host entries to ~/.ssh/config and rescan.",
    favorites: "Star a host to pin it here.",
    recent: "Hosts you connect to will show up here.",
    removed: "Hosts that disappear from ~/.ssh/config are listed here.",
    trash: "Trash is empty.",
    group: "This group has no hosts. Use “Move to group” on a host.",
  };
  return (
    <Empty icon={ServerIcon} title="Nothing here" text={text[view]}>
      {(view === "all" || view === "group") && (
        <Button size="sm" variant="outline" onClick={() => openHostForm("new")}>
          <PlusIcon /> New host
        </Button>
      )}
    </Empty>
  );
}

function Empty({
  icon: Icon,
  title,
  text,
  children,
}: {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  text: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center">
      <Icon className="size-10 text-muted-foreground" />
      <div>
        <p className="font-medium">{title}</p>
        <p className="max-w-sm text-sm text-muted-foreground">{text}</p>
      </div>
      {children}
    </div>
  );
}

function EmptyTrashButton({ hosts }: { hosts: Host[] }) {
  const [open, setOpen] = useState(false);
  const empty = useEmptyTrash();
  const inConfig = hosts.filter(stillInConfig).length;
  const purgeable = hosts.length - inConfig;
  return (
    <>
      <Button
        size="sm"
        variant="ghost"
        className="ml-auto text-destructive"
        disabled={purgeable === 0}
        title={purgeable === 0 ? "Every host here is still in ~/.ssh/config" : undefined}
        onClick={() => setOpen(true)}
      >
        <Trash2Icon /> Empty trash
      </Button>
      <ConfirmDialog
        open={open}
        onOpenChange={setOpen}
        title={`Permanently delete ${purgeable} host${purgeable === 1 ? "" : "s"}?`}
        description={
          <>
            Their notes, tags and custom fields are removed. The change history keeps a copy, so
            they can still be brought back from History.
            {inConfig > 0 &&
              ` ${inConfig} host${inConfig === 1 ? " is" : "s are"} still in ~/.ssh/config and stay in trash (remove ${inConfig === 1 ? "it" : "them"} from the config with Write config first).`}
          </>
        }
        confirmLabel="Delete permanently"
        destructive
        onConfirm={() => {
          empty.mutate();
          setOpen(false);
        }}
      />
    </>
  );
}
