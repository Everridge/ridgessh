import {
  ArrowDownToLineIcon,
  ArrowUpFromLineIcon,
  ChevronRightIcon,
  CircleAlertIcon,
  CircleCheckIcon,
  CircleIcon,
  CircleXIcon,
  FileCode2Icon,
  FolderDownIcon,
  LoaderCircleIcon,
  PlusIcon,
  RotateCwIcon,
  SquareIcon,
  SquareTerminalIcon,
  XIcon,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/ui";
import type { StepStatus } from "@/lib/ipc";
import {
  resumableRun,
  useTerminalStore,
  type FolderStep,
  type TabStatus,
  type TerminalTab,
} from "./store";
import { TerminalView } from "./terminal-view";
import { platform } from "@/lib/platform";

const STATUS_DOT: Record<TabStatus, string> = {
  connecting: "bg-amber-400 animate-pulse",
  open: "bg-emerald-500",
  exited: "bg-muted-foreground/50",
  error: "bg-destructive",
};

export function TerminalPanel() {
  const { tabs, activeId, activate, close, reconnect, stop } = useTerminalStore();
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
              <JobIcon tab={t} />
              <span className="truncate">{t.title}</span>
              {t.job.kind === "transfer" && t.progress !== null && (
                <span className="shrink-0 text-muted-foreground tabular-nums">{t.progress}%</span>
              )}
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
        {active && active.job.kind !== "ssh" && active.status === "open" && (
          <Button variant="ghost" size="xs" onClick={() => stop(active.id)}>
            <SquareIcon /> Stop
          </Button>
        )}
        {active && (active.status === "exited" || active.status === "error") && (
          <Button variant="ghost" size="xs" onClick={() => reconnect(active.id)}>
            <RotateCwIcon />{" "}
            {active.job.kind === "ssh" ? "Reconnect" : resumableRun(active) ? "Retry" : "Run again"}
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
          <TooltipContent>New session ({platform.shortcut("K")})</TooltipContent>
        </Tooltip>
      </div>

      {active?.run && <StepStrip steps={active.run.steps} />}
      <div className="relative min-h-0 flex-1 overflow-hidden">
        {tabs.length === 0 ? (
          <div className="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground">
            <SquareTerminalIcon className="size-4" />
            No open sessions. Double-click a host or press {platform.shortcut("K")}.
          </div>
        ) : (
          tabs.map((t) => <TerminalView key={t.id} tab={t} active={t.id === activeId} />)
        )}
      </div>
    </section>
  );
}

const STEP_ICON: Record<StepStatus, React.ComponentType<{ className?: string }>> = {
  pending: CircleIcon,
  running: LoaderCircleIcon,
  done: CircleCheckIcon,
  warning: CircleAlertIcon,
  failed: CircleXIcon,
  skipped: CircleCheckIcon,
};

const STEP_COLOR: Record<StepStatus, string> = {
  pending: "text-muted-foreground",
  running: "text-foreground animate-spin",
  done: "text-emerald-600",
  warning: "text-amber-600",
  failed: "text-destructive",
  skipped: "text-muted-foreground",
};

/** A folder transfer's steps and where it is. */
function StepStrip({ steps }: { steps: FolderStep[] }) {
  return (
    <ol
      aria-label="Steps"
      className="flex shrink-0 items-center gap-1 overflow-x-auto border-b px-2 py-1 text-xs"
    >
      {steps.map((s, i) => {
        const Icon = STEP_ICON[s.status];
        return (
          <li
            key={i}
            title={s.detail ?? undefined}
            aria-label={`${s.label}: ${s.status}`}
            className={cn(
              "flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5",
              s.status === "running" && "bg-muted font-medium",
              s.status === "failed" && "bg-destructive/10",
            )}
          >
            {i > 0 && <ChevronRightIcon className="-ml-1 size-3 text-muted-foreground/60" />}
            <Icon className={cn("size-3.5", STEP_COLOR[s.status])} />
            <span className={s.status === "skipped" ? "text-muted-foreground line-through" : ""}>
              {s.label}
            </span>
          </li>
        );
      })}
    </ol>
  );
}

function JobIcon({ tab }: { tab: TerminalTab }) {
  const cls = "size-3 shrink-0 text-muted-foreground";
  if (tab.job.kind === "folder") return <FolderDownIcon className={cls} aria-label="Folder" />;
  if (tab.job.kind === "script") return <FileCode2Icon className={cls} aria-label="Script" />;
  if (tab.job.kind === "transfer")
    return tab.job.spec.direction === "upload" ? (
      <ArrowUpFromLineIcon className={cls} aria-label="Upload" />
    ) : (
      <ArrowDownToLineIcon className={cls} aria-label="Download" />
    );
  return null;
}
