import { Badge } from "@/components/ui/badge";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { Host } from "@/lib/ipc";

const STATE: Record<string, { label: string; hint: string; className: string } | undefined> = {
  db_only: {
    label: "DB only",
    hint: "Exists in RidgeSSH but not in ~/.ssh/config",
    className: "text-muted-foreground",
  },
  config_only_new: {
    label: "New",
    hint: "Found in ~/.ssh/config on the last scan",
    className: "border-sky-500/40 text-sky-700 dark:text-sky-400",
  },
  removed_from_config: {
    label: "Removed",
    hint: "No longer present in ~/.ssh/config",
    className: "border-amber-500/40 text-amber-700 dark:text-amber-400",
  },
  conflict: {
    label: "Conflict",
    hint: "Changed both here and in ~/.ssh/config",
    className: "border-destructive/40 text-destructive",
  },
};

export function SyncBadges({ host }: { host: Host }) {
  const s = STATE[host.syncState];
  return (
    <>
      {s && <HintBadge label={s.label} hint={s.hint} className={s.className} />}
      {host.configDirty && (
        <HintBadge
          label="Pending"
          hint="Connection changes not yet written to ~/.ssh/config"
          className="border-violet-500/40 text-violet-700 dark:text-violet-400"
        />
      )}
    </>
  );
}

function HintBadge({ label, hint, className }: { label: string; hint: string; className: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge variant="outline" className={className}>
          {label}
        </Badge>
      </TooltipTrigger>
      <TooltipContent>{hint}</TooltipContent>
    </Tooltip>
  );
}
