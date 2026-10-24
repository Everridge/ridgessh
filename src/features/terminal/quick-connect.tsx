import { useMemo } from "react";
import { ClockIcon, ServerIcon, StarIcon } from "lucide-react";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "@/components/ui/command";
import { groupPath } from "@/features/hosts/filter";
import { connectionLabel } from "@/features/hosts/format";
import { useGroupsData, useHostsData } from "@/features/hosts/queries";
import type { Group, Host } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { useConnect } from "./use-connect";

export function QuickConnect() {
  const open = useUiStore((s) => s.quickConnectOpen);
  const setOpen = useUiStore((s) => s.setQuickConnectOpen);
  const hosts = useHostsData();
  const groups = useGroupsData();
  const { connect } = useConnect();

  const { favorites, recent, rest } = useMemo(() => {
    const live = hosts.filter((h) => h.deletedAt === null);
    const recent = live
      .filter((h) => h.lastConnectedAt && !h.favorite)
      .sort((a, b) => (b.lastConnectedAt ?? "").localeCompare(a.lastConnectedAt ?? ""))
      .slice(0, 5);
    const shown = new Set(recent.map((h) => h.id));
    return {
      favorites: live.filter((h) => h.favorite),
      recent,
      rest: live.filter((h) => !h.favorite && !shown.has(h.id)),
    };
  }, [hosts]);

  const pick = (h: Host) => {
    setOpen(false);
    connect(h);
  };

  return (
    <CommandDialog
      open={open}
      onOpenChange={setOpen}
      title="Quick connect"
      description="Search hosts and open an SSH session"
    >
      <Command>
        <CommandInput placeholder="Connect to… (alias, hostname, user, tag, group)" />
        <CommandList>
          <CommandEmpty>No hosts found.</CommandEmpty>
          <HostGroup
            heading="Favorites"
            icon={StarIcon}
            hosts={favorites}
            groups={groups}
            onPick={pick}
          />
          <HostGroup
            heading="Recent"
            icon={ClockIcon}
            hosts={recent}
            groups={groups}
            onPick={pick}
          />
          <HostGroup
            heading="All hosts"
            icon={ServerIcon}
            hosts={rest}
            groups={groups}
            onPick={pick}
          />
        </CommandList>
      </Command>
    </CommandDialog>
  );
}

function HostGroup({
  heading,
  icon: Icon,
  hosts,
  groups,
  onPick,
}: {
  heading: string;
  icon: React.ComponentType;
  hosts: Host[];
  groups: Group[];
  onPick: (h: Host) => void;
}) {
  if (!hosts.length) return null;
  return (
    <CommandGroup heading={heading}>
      {hosts.map((h) => {
        const path = groupPath(groups, h.groupId);
        return (
          <CommandItem
            key={h.id}
            // cmdk filters on value + keywords.
            value={`${h.alias} ${h.id}`}
            keywords={[
              h.hostname ?? "",
              h.user ?? "",
              path,
              h.description,
              ...h.tags.map((t) => t.name),
            ]}
            onSelect={() => onPick(h)}
          >
            <Icon />
            <span className="font-medium">{h.alias}</span>
            <span className="truncate text-muted-foreground">{connectionLabel(h)}</span>
            {path && <CommandShortcut>{path}</CommandShortcut>}
          </CommandItem>
        );
      })}
    </CommandGroup>
  );
}
