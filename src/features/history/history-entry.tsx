import {
  ArchiveRestoreIcon,
  HistoryIcon,
  PencilIcon,
  PlusIcon,
  RotateCcwIcon,
  Trash2Icon,
  Undo2Icon,
  XCircleIcon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { groupPath } from "@/features/hosts/filter";
import { formatTs } from "@/features/hosts/format";
import { useGroupsData } from "@/features/hosts/queries";
import type { AuditEntry } from "@/lib/ipc";
import { diffSnapshots, fieldLabel } from "./diff";
import { canRevert, SOURCE_LABELS, useRevert } from "./queries";

type Icon = React.ComponentType<{ className?: string }>;

function describe(e: AuditEntry): { label: string; icon: Icon } {
  const host = e.entityType === "host";
  switch (e.action) {
    case "create":
      return { label: "Created", icon: PlusIcon };
    case "update":
      return { label: "Updated", icon: PencilIcon };
    case "delete":
      return { label: host ? "Moved to trash" : "Deleted", icon: Trash2Icon };
    case "restore":
      return e.entityType === "backup"
        ? { label: "Backup restored", icon: ArchiveRestoreIcon }
        : { label: "Restored from trash", icon: RotateCcwIcon };
    case "purge":
      return { label: "Deleted permanently", icon: XCircleIcon };
  }
}

/** What "undo" does for this kind of change, in words. */
function undoLabel(e: AuditEntry): string {
  if (e.action === "create")
    return e.entityType === "host" ? "Undo (move to trash)" : "Undo (delete)";
  if (e.action === "delete" || e.action === "purge") return "Bring back";
  return "Undo";
}

export function HistoryEntryIcon({ entry }: { entry: AuditEntry }) {
  const { icon: I } = describe(entry);
  return (
    <span className="z-10 flex size-6 shrink-0 items-center justify-center rounded-full border bg-background">
      <I className="size-3.5 text-muted-foreground" />
    </span>
  );
}

/**
 * One audit entry: what happened, which fields changed, and undo / restore actions.
 * `showLabel` adds the entity name (for the global history).
 */
export function HistoryEntry({ entry: e, showLabel }: { entry: AuditEntry; showLabel?: boolean }) {
  const groups = useGroupsData();
  const revert = useRevert();
  const { label } = describe(e);
  const changes = e.action === "update" ? diffSnapshots(e.before, e.after) : [];
  const fmt = (field: string, v: string) =>
    (field === "groupId" || field === "parentId") && v !== "—"
      ? groupPath(groups, v) || "(deleted group)"
      : v;
  const revertable = canRevert(e);

  return (
    <div className="group/entry min-w-0 flex-1">
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
        {showLabel && <span className="font-medium">{e.label}</span>}
        <span className={showLabel ? "text-muted-foreground" : "font-medium"}>{label}</span>
        {e.source !== "user" && <Badge variant="outline">{SOURCE_LABELS[e.source]}</Badge>}
        <span className="text-xs text-muted-foreground">{formatTs(e.ts)}</span>
        {revertable && (
          <span className="ml-auto flex gap-1 opacity-60 transition-opacity group-hover/entry:opacity-100 focus-within:opacity-100">
            <Button
              size="xs"
              variant="ghost"
              disabled={revert.isPending}
              onClick={() => revert.mutate({ id: e.id, target: "before" })}
              title="Revert what this change did; later changes are kept"
            >
              <Undo2Icon /> {undoLabel(e)}
            </Button>
            {e.action === "update" && (
              <Button
                size="xs"
                variant="ghost"
                disabled={revert.isPending}
                onClick={() => revert.mutate({ id: e.id, target: "after" })}
                title="Make it exactly as it was right after this change"
              >
                <HistoryIcon /> Restore this version
              </Button>
            )}
          </span>
        )}
      </div>
      {changes.length > 0 && (
        <dl className="mt-1 grid gap-0.5 text-xs">
          {changes.map((c) => (
            <div key={c.field} className="flex min-w-0 gap-1.5">
              <dt className="shrink-0 text-muted-foreground">{fieldLabel(c.field)}:</dt>
              <dd className="min-w-0 truncate">
                <span className="text-muted-foreground line-through">{fmt(c.field, c.before)}</span>{" "}
                → {fmt(c.field, c.after)}
              </dd>
            </div>
          ))}
        </dl>
      )}
    </div>
  );
}
