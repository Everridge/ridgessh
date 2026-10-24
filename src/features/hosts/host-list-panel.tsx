import { useMemo, useState } from "react";
import { ConfirmDialog } from "@/components/confirm-dialog";
import {
  DownloadIcon,
  ExternalLinkIcon,
  FileDownIcon,
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
import type { Group, Host } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { useUiStore, type HostView } from "@/stores/ui";
import { buildGroupTree, filterHosts, flattenTree, groupPath } from "./filter";
import { connectionLabel } from "./format";
import { useHostActions, usePurgeConfirm } from "./host-actions";
import { SyncBadges } from "./host-badges";
import { useEmptyTrash, useGroupsData, useHosts, useMoveHosts, useSetFavorite } from "./queries";
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
  const { view, groupId, search, checkedIds, setChecked, openHostForm } = useUiStore();
  const actions = useHostActions();
  const move = useMoveHosts();

  const hosts = useMemo(
    () => filterHosts(hostsQuery.data ?? [], groups, { view, groupId, search }),
    [hostsQuery.data, groups, view, groupId, search],
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
              <Button size="sm" className="ml-auto" onClick={() => openHostForm("new")}>
                <PlusIcon /> New host
              </Button>
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
            {hosts.map((h) => (
              <HostRow key={h.id} host={h} groups={groups} />
            ))}
          </ul>
        </ScrollArea>
      )}
    </section>
  );
}

function HostRow({ host, groups }: { host: Host; groups: Group[] }) {
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
      {purge.dialog}
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
