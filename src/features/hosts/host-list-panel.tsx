import { useQuery } from "@tanstack/react-query";
import { CheckCircle2Icon, FileWarningIcon, ServerIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { api } from "@/lib/ipc";

export function HostListPanel() {
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.appInfo });

  return (
    <section className="flex h-full flex-col">
      <div className="flex h-10 items-center justify-between border-b px-3">
        <h2 className="text-sm font-medium">Hosts</h2>
        <Badge variant="secondary">0</Badge>
      </div>
      <div className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center">
        <ServerIcon className="size-10 text-muted-foreground" />
        <div>
          <p className="font-medium">No hosts yet</p>
          <p className="text-sm text-muted-foreground">
            Hosts will be imported from your SSH config on startup.
          </p>
        </div>
        {info.data && (
          <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
            {info.data.sshConfigExists ? (
              <CheckCircle2Icon className="size-3.5 text-emerald-600" />
            ) : (
              <FileWarningIcon className="size-3.5 text-amber-600" />
            )}
            <code>{info.data.sshConfig}</code>
            {!info.data.sshConfigExists && " (not found)"}
          </p>
        )}
        {info.error && (
          <p className="text-xs text-destructive">Backend unavailable: {String(info.error)}</p>
        )}
      </div>
    </section>
  );
}
