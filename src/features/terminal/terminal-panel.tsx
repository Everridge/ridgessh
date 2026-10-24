import { PlusIcon, RotateCwIcon, SquareTerminalIcon, XIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/ui";
import { useTerminalStore, type TabStatus } from "./store";
import { TerminalView } from "./terminal-view";

const STATUS_DOT: Record<TabStatus, string> = {
  connecting: "bg-amber-400 animate-pulse",
  open: "bg-emerald-500",
  exited: "bg-muted-foreground/50",
  error: "bg-destructive",
};

export function TerminalPanel() {
  const { tabs, activeId, activate, close, reconnect } = useTerminalStore();
  const openQuickConnect = useUiStore((s) => s.setQuickConnectOpen);
  const active = tabs.find((t) => t.id === activeId);

  return (
    <section className="flex h-full flex-col bg-muted/30">
      <div className="flex h-9 shrink-0 items-center gap-1 border-b pr-1 pl-2">
        <div
          role="tablist"
          aria-label="Terminal sessions"
          className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto"
        >
          {tabs.length === 0 && (
            <h2 className="px-1 text-xs font-medium tracking-wide text-muted-foreground uppercase">
              Terminal
            </h2>
          )}
          {tabs.map((t) => (
            <div
              key={t.id}
              role="tab"
              aria-selected={t.id === activeId}
              tabIndex={0}
              onClick={() => activate(t.id)}
              onKeyDown={(e) => e.key === "Enter" && activate(t.id)}
              onAuxClick={(e) => e.button === 1 && close(t.id)}
              className={cn(
                "group flex h-7 max-w-48 shrink-0 cursor-default items-center gap-1.5 rounded-md pr-1 pl-2 text-xs",
                t.id === activeId
                  ? "bg-background shadow-sm"
                  : "text-muted-foreground hover:bg-muted",
              )}
            >
              <span className={cn("size-1.5 shrink-0 rounded-full", STATUS_DOT[t.status])} />
              <span className="truncate">{t.title}</span>
              <button
                type="button"
                aria-label={`Close ${t.title}`}
                onClick={(e) => {
                  e.stopPropagation();
                  close(t.id);
                }}
                className="rounded p-0.5 opacity-60 hover:bg-muted hover:opacity-100"
              >
                <XIcon className="size-3" />
              </button>
            </div>
          ))}
        </div>
        {active && (active.status === "exited" || active.status === "error") && (
          <Button variant="ghost" size="xs" onClick={() => reconnect(active.id)}>
            <RotateCwIcon /> Reconnect
          </Button>
        )}
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label="New session"
              onClick={() => openQuickConnect(true)}
            >
              <PlusIcon />
            </Button>
          </TooltipTrigger>
          <TooltipContent>New session (Ctrl+K)</TooltipContent>
        </Tooltip>
      </div>

      <div className="relative min-h-0 flex-1">
        {tabs.length === 0 ? (
          <div className="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground">
            <SquareTerminalIcon className="size-4" />
            No open sessions. Double-click a host or press Ctrl+K.
          </div>
        ) : (
          tabs.map((t) => <TerminalView key={t.id} tab={t} active={t.id === activeId} />)
        )}
      </div>
    </section>
  );
}
