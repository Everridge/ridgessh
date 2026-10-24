import {
  ArchiveRestoreIcon,
  ClockIcon,
  FolderPlusIcon,
  HistoryIcon,
  ServerIcon,
  SettingsIcon,
  StarIcon,
  Trash2Icon,
  TriangleAlertIcon,
} from "lucide-react";
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
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";
import { useUiStore, type HostView } from "@/stores/ui";

const views: { id: HostView; label: string; icon: React.ComponentType }[] = [
  { id: "all", label: "All hosts", icon: ServerIcon },
  { id: "favorites", label: "Favorites", icon: StarIcon },
  { id: "recent", label: "Recent", icon: ClockIcon },
  { id: "removed", label: "Removed from config", icon: TriangleAlertIcon },
  { id: "trash", label: "Trash", icon: Trash2Icon },
];

export function AppSidebar() {
  const view = useUiStore((s) => s.view);
  const setView = useUiStore((s) => s.setView);

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
              {views.map((v) => (
                <SidebarMenuItem key={v.id}>
                  <SidebarMenuButton
                    isActive={view === v.id}
                    onClick={() => setView(v.id)}
                    tooltip={v.label}
                  >
                    <v.icon />
                    <span>{v.label}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>

        <SidebarGroup>
          <SidebarGroupLabel>Groups</SidebarGroupLabel>
          <SidebarGroupAction title="New group" disabled>
            <FolderPlusIcon />
          </SidebarGroupAction>
          <SidebarGroupContent>
            <p className="px-2 py-1 text-xs text-muted-foreground group-data-[collapsible=icon]:hidden">
              No groups yet.
            </p>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      <SidebarFooter>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton tooltip="History" disabled>
              <HistoryIcon />
              <span>History</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
          <SidebarMenuItem>
            <SidebarMenuButton tooltip="Backups" disabled>
              <ArchiveRestoreIcon />
              <span>Backups</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
          <SidebarMenuItem>
            <SidebarMenuButton tooltip="Settings" disabled>
              <SettingsIcon />
              <span>Settings</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  );
}
