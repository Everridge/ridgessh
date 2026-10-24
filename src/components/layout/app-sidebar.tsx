import { useMemo, useState } from "react";
import {
  ArchiveRestoreIcon,
  ArrowDownUpIcon,
  DownloadIcon,
  UploadIcon,
  ChevronRightIcon,
  ClockIcon,
  FolderIcon,
  FolderPlusIcon,
  FileCode2Icon,
  HistoryIcon,
  MoreHorizontalIcon,
  PencilIcon,
  ServerIcon,
  SettingsIcon,
  StarIcon,
  Trash2Icon,
  TriangleAlertIcon,
} from "lucide-react";
import { ConfirmDialog } from "@/components/confirm-dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupAction,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";
import { GroupDialog, type GroupDialogTarget } from "@/features/groups/group-dialog";
import { buildGroupTree, countByGroup, type GroupNode } from "@/features/hosts/filter";
import { useBackupStatus } from "@/features/backup/queries";
import { useDeleteGroup, useGroupsData, useHostsData } from "@/features/hosts/queries";
import type { Group, Host } from "@/lib/ipc";
import { useUiStore, type HostView } from "@/stores/ui";

type FixedView = Exclude<HostView, "group">;

const views: {
  id: FixedView;
  label: string;
  icon: React.ComponentType;
  count: (h: Host) => boolean;
}[] = [
  { id: "all", label: "All hosts", icon: ServerIcon, count: (h) => !h.deletedAt },
  { id: "favorites", label: "Favorites", icon: StarIcon, count: (h) => !h.deletedAt && h.favorite },
  {
    id: "recent",
    label: "Recent",
    icon: ClockIcon,
    count: (h) => !h.deletedAt && !!h.lastConnectedAt,
  },
  {
    id: "removed",
    label: "Removed from config",
    icon: TriangleAlertIcon,
    count: (h) => !h.deletedAt && h.syncState === "removed_from_config",
  },
  { id: "trash", label: "Trash", icon: Trash2Icon, count: (h) => !!h.deletedAt },
];

export function AppSidebar() {
  const view = useUiStore((s) => s.view);
  const setView = useUiStore((s) => s.setView);
  const hosts = useHostsData();
  const groups = useGroupsData();
  const [dialog, setDialog] = useState<GroupDialogTarget | null>(null);
  const [deleting, setDeleting] = useState<Group | null>(null);
  const deleteGroup = useDeleteGroup();
  const setBackupsOpen = useUiStore((s) => s.setBackupsOpen);
  const setSettingsOpen = useUiStore((s) => s.setSettingsOpen);
  const setHistoryOpen = useUiStore((s) => s.setHistoryOpen);
  const backup = useBackupStatus().data;
  const backupWarning = !backup
    ? null
    : backup.lastError
      ? "The last automatic backup failed"
      : !backup.passwordRemembered
        ? "Automatic backups are off: no backup password"
        : null;

  const tree = useMemo(() => buildGroupTree(groups), [groups]);
  const counts = useMemo(() => countByGroup(hosts, groups), [hosts, groups]);

  return (
    <Sidebar collapsible="icon">
      <SidebarHeader>
        <div className="flex items-center gap-2 px-2 py-1.5">
          <div className="flex size-7 shrink-0 items-center justify-center rounded-md bg-primary text-xs font-bold text-primary-foreground">
            R
          </div>
          <span className="truncate font-semibold group-data-[collapsible=icon]:hidden">
            RidgeSSH
          </span>
        </div>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupContent>
            <SidebarMenu>
              {views.map((v) => {
                const n = hosts.filter(v.count).length;
                return (
                  <SidebarMenuItem key={v.id}>
                    <SidebarMenuButton
                      isActive={view === v.id}
                      onClick={() => setView(v.id)}
                      tooltip={v.label}
                    >
                      <v.icon />
                      <span className="flex-1 truncate">{v.label}</span>
                      {n > 0 && (
                        <span className="text-xs text-muted-foreground tabular-nums">{n}</span>
                      )}
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                );
              })}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>

        <SidebarGroup>
          <SidebarGroupLabel>Groups</SidebarGroupLabel>
          <SidebarGroupAction
            title="New group"
            onClick={() => setDialog({ mode: "create", parentId: null })}
          >
            <FolderPlusIcon />
          </SidebarGroupAction>
          <SidebarGroupContent>
            {tree.length === 0 ? (
              <p className="px-2 py-1 text-xs text-muted-foreground group-data-[collapsible=icon]:hidden">
                No groups yet.
              </p>
            ) : (
              <SidebarMenu>
                {tree.map((node) => (
                  <GroupItem
                    key={node.id}
                    node={node}
                    counts={counts}
                    onCreateChild={(parentId) => setDialog({ mode: "create", parentId })}
                    onEdit={(group) => setDialog({ mode: "edit", group })}
                    onDelete={setDeleting}
                  />
                ))}
              </SidebarMenu>
            )}
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      <SidebarFooter>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              tooltip="Scripts"
              onClick={() => useUiStore.getState().setScriptsOpen(true)}
            >
              <FileCode2Icon />
              <span>Scripts</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
          <SidebarMenuItem>
            <SidebarMenuButton tooltip="History" onClick={() => setHistoryOpen(true)}>
              <HistoryIcon />
              <span>History</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
          <SidebarMenuItem>
            <SidebarMenuButton tooltip="Backups" onClick={() => setBackupsOpen(true)}>
              <ArchiveRestoreIcon />
              <span>Backups</span>
              {backupWarning && (
                <span
                  className="ml-auto size-2 rounded-full bg-amber-500"
                  aria-label={backupWarning}
                  title={backupWarning}
                />
              )}
            </SidebarMenuButton>
          </SidebarMenuItem>
          <SidebarMenuItem>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <SidebarMenuButton tooltip="Import / export">
                  <ArrowDownUpIcon />
                  <span>Import / export</span>
                </SidebarMenuButton>
              </DropdownMenuTrigger>
              <DropdownMenuContent side="right" align="end">
                <DropdownMenuItem onSelect={() => useUiStore.getState().setImportOpen(true)}>
                  <UploadIcon /> Import…
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => useUiStore.getState().openExport()}>
                  <DownloadIcon /> Export…
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </SidebarMenuItem>
          <SidebarMenuItem>
            <SidebarMenuButton tooltip="Settings" onClick={() => setSettingsOpen(true)}>
              <SettingsIcon />
              <span>Settings</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>

      <GroupDialog target={dialog} onClose={() => setDialog(null)} />
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(o) => !o && setDeleting(null)}
        title={`Delete group "${deleting?.name}"?`}
        description="Its hosts and subgroups move to the parent group. No hosts are deleted."
        confirmLabel="Delete group"
        destructive
        onConfirm={() => {
          if (deleting) deleteGroup.mutate(deleting.id);
          setDeleting(null);
        }}
      />
    </Sidebar>
  );
}

interface GroupItemProps {
  node: GroupNode;
  counts: Map<string, number>;
  onCreateChild: (parentId: string) => void;
  onEdit: (group: Group) => void;
  onDelete: (group: Group) => void;
}

function GroupItem({ node, counts, onCreateChild, onEdit, onDelete }: GroupItemProps) {
  const active = useUiStore((s) => s.view === "group" && s.groupId === node.id);
  const setGroup = useUiStore((s) => s.setGroup);
  const [open, setOpen] = useState(true);
  const n = counts.get(node.id) ?? 0;

  return (
    <>
      <SidebarMenuItem>
        <SidebarMenuButton
          isActive={active}
          onClick={() => setGroup(node.id)}
          tooltip={node.name}
          style={{ paddingLeft: 8 + node.depth * 14 }}
        >
          {node.children.length > 0 ? (
            <ChevronRightIcon
              role="button"
              aria-label={open ? "Collapse" : "Expand"}
              className={`transition-transform ${open ? "rotate-90" : ""}`}
              onClick={(e) => {
                e.stopPropagation();
                setOpen(!open);
              }}
            />
          ) : (
            <FolderIcon style={node.color ? { color: node.color } : undefined} />
          )}
          <span className="flex-1 truncate">{node.name}</span>
          {n > 0 && (
            <span className="text-xs text-muted-foreground tabular-nums group-hover/menu-item:invisible">
              {n}
            </span>
          )}
        </SidebarMenuButton>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <SidebarMenuAction showOnHover>
              <MoreHorizontalIcon />
              <span className="sr-only">Group actions</span>
            </SidebarMenuAction>
          </DropdownMenuTrigger>
          <DropdownMenuContent side="right" align="start">
            <DropdownMenuItem onSelect={() => onCreateChild(node.id)}>
              <FolderPlusIcon /> New subgroup
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => onEdit(node)}>
              <PencilIcon /> Edit
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem variant="destructive" onSelect={() => onDelete(node)}>
              <Trash2Icon /> Delete
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
      {open &&
        node.children.map((child) => (
          <GroupItem
            key={child.id}
            node={child}
            counts={counts}
            onCreateChild={onCreateChild}
            onEdit={onEdit}
            onDelete={onDelete}
          />
        ))}
    </>
  );
}
