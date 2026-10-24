import { PencilIcon, PlusIcon, RotateCcwIcon, Trash2Icon, XCircleIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { useGroupsData, useHostHistory } from "@/features/hosts/queries";
import { groupPath } from "@/features/hosts/filter";
import { formatTs } from "@/features/hosts/format";
import type { AuditEntry } from "@/lib/ipc";
import { diffSnapshots, fieldLabel } from "./diff";

const ACTIONS: Record<
  AuditEntry["action"],
  { label: string; icon: React.ComponentType<{ className?: string }> }
> = {
  create: { label: "Created", icon: PlusIcon },
  update: { label: "Updated", icon: PencilIcon },
  delete: { label: "Moved to trash", icon: Trash2Icon },
  restore: { label: "Restored", icon: RotateCcwIcon },
  purge: { label: "Deleted permanently", icon: XCircleIcon },
};

export function HostHistory({ hostId }: { hostId: string }) {
  const history = useHostHistory(hostId);
  const groups = useGroupsData();

  if (history.isLoading) return null;
  if (!history.data?.length) {
    return <p className="p-4 text-sm text-muted-foreground">No history yet.</p>;
  }

  return (
    <ol className="relative grid gap-4 p-4 before:absolute before:top-6 before:bottom-6 before:left-[27px] before:w-px before:bg-border">
      {history.data.map((e) => {
        const a = ACTIONS[e.action];
        const changes = e.action === "update" ? diffSnapshots(e.before, e.after) : [];
        return (
          <li key={e.id} className="relative flex gap-3">
            <span className="z-10 flex size-6 shrink-0 items-center justify-center rounded-full border bg-background">
              <a.icon className="size-3.5 text-muted-foreground" />
            </span>
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center gap-2 text-sm">
                <span className="font-medium">{a.label}</span>
                {e.source !== "user" && <Badge variant="outline">{e.source}</Badge>}
                <span className="text-xs text-muted-foreground">{formatTs(e.ts)}</span>
              </div>
              {changes.length > 0 && (
                <dl className="mt-1 grid gap-0.5 text-xs">
                  {changes.map((c) => {
                    const fmt = (v: string) =>
                      c.field === "groupId" && v !== "—"
                        ? groupPath(groups, v) || "(deleted group)"
                        : v;
                    return (
                      <div key={c.field} className="flex min-w-0 gap-1.5">
                        <dt className="shrink-0 text-muted-foreground">{fieldLabel(c.field)}:</dt>
                        <dd className="min-w-0 truncate">
                          <span className="text-muted-foreground line-through">
                            {fmt(c.before)}
                          </span>{" "}
                          → {fmt(c.after)}
                        </dd>
                      </div>
                    );
                  })}
                </dl>
              )}
            </div>
          </li>
        );
      })}
    </ol>
  );
}
