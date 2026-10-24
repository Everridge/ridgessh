import { useQuery } from "@tanstack/react-query";
import { ExternalLinkIcon, SquareTerminalIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { duration, formatTs } from "@/features/hosts/format";
import { api, type Activity } from "@/lib/ipc";

export function HostSessions({ hostId }: { hostId: string }) {
  // Shares the "host-history" prefix so every data change refreshes it too.
  const q = useQuery({
    queryKey: ["host-history", "activity", hostId],
    queryFn: () => api.hostActivity(hostId),
  });
  if (q.isLoading) return null;
  if (!q.data?.length) {
    return <p className="p-4 text-sm text-muted-foreground">No sessions yet.</p>;
  }
  return (
    <ul className="divide-y">
      {q.data.map((a) => (
        <SessionRow key={a.id} a={a} />
      ))}
    </ul>
  );
}

function SessionRow({ a }: { a: Activity }) {
  const external = a.kind === "terminal_external";
  const Icon = external ? ExternalLinkIcon : SquareTerminalIcon;
  return (
    <li className="flex items-center gap-3 px-4 py-2 text-sm">
      <Icon className="size-4 shrink-0 text-muted-foreground" />
      <div className="min-w-0 flex-1">
        <div>{formatTs(a.startedAt)}</div>
        {a.details?.command && (
          <code className="block truncate text-xs text-muted-foreground">{a.details.command}</code>
        )}
      </div>
      {external ? (
        <Badge variant="outline">external</Badge>
      ) : (
        <>
          <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
            {duration(a.startedAt, a.endedAt)}
          </span>
          {a.exitCode !== null && (
            <Badge variant={a.exitCode === 0 ? "secondary" : "destructive"}>
              exit {a.exitCode}
            </Badge>
          )}
        </>
      )}
    </li>
  );
}
