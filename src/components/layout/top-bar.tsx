import {
  FileCheckIcon,
  MonitorIcon,
  MoonIcon,
  RefreshCwIcon,
  SearchIcon,
  SunIcon,
} from "lucide-react";
import { toast } from "sonner";
import { useTheme } from "@/components/theme-provider";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useHostsData } from "@/features/hosts/queries";
import { reviewItems, summarize } from "@/features/sync/compare";
import { useRenameSuggestions, useRunSync } from "@/features/sync/queries";
import { cn } from "@/lib/utils";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { SidebarTrigger } from "@/components/ui/sidebar";
import { useUiStore } from "@/stores/ui";

export function TopBar() {
  const search = useUiStore((s) => s.search);
  const setSearch = useUiStore((s) => s.setSearch);

  return (
    <header className="flex h-12 shrink-0 items-center gap-2 border-b px-3">
      <SidebarTrigger />
      <Separator orientation="vertical" className="h-5" />
      <div className="relative max-w-md flex-1">
        <SearchIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search hosts…"
          className="h-8 pl-8"
          aria-label="Search hosts"
        />
      </div>
      <div className="ml-auto flex items-center gap-1">
        <SyncControls />
        <ThemeToggle />
      </div>
    </header>
  );
}

function SyncControls() {
  const hosts = useHostsData();
  const renames = useRenameSuggestions().data ?? [];
  const rescan = useRunSync();
  const openReview = useUiStore((s) => s.setSyncReviewOpen);
  const items = reviewItems(hosts);
  const attention =
    items.added.length + items.removed.length + items.conflicts.length + renames.length;

  return (
    <>
      <Button variant="ghost" size="sm" onClick={() => openReview(true)}>
        <FileCheckIcon />
        Config review
        {attention > 0 && (
          <Badge
            variant={items.conflicts.length ? "destructive" : "secondary"}
            className="h-4 px-1 tabular-nums"
          >
            {attention}
          </Badge>
        )}
      </Button>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="ghost"
            size="icon"
            aria-label="Rescan SSH config"
            disabled={rescan.isPending}
            onClick={() =>
              rescan.mutate(undefined, {
                onSuccess: (r) => toast.success(`Config scanned: ${summarize(r) || "no changes"}`),
              })
            }
          >
            <RefreshCwIcon className={cn(rescan.isPending && "animate-spin")} />
          </Button>
        </TooltipTrigger>
        <TooltipContent>Rescan ~/.ssh/config</TooltipContent>
      </Tooltip>
    </>
  );
}

function ThemeToggle() {
  const { theme, setTheme } = useTheme();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" aria-label="Theme">
          <SunIcon className="dark:hidden" />
          <MoonIcon className="hidden dark:block" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuRadioGroup value={theme} onValueChange={setTheme}>
          <DropdownMenuRadioItem value="light">
            <SunIcon /> Light
          </DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="dark">
            <MoonIcon /> Dark
          </DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="system">
            <MonitorIcon /> System
          </DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
