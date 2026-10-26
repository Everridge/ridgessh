import { useQuery } from "@tanstack/react-query";
import { DownloadIcon, LoaderCircleIcon, TriangleAlertIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useTerminalStore } from "@/features/terminal/store";
import { api } from "@/lib/ipc";
import { useUpdateStore } from "./store";

const mb = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;

export function UpdateDialog() {
  const open = useUpdateStore((s) => s.dialogOpen);
  const setOpen = useUpdateStore((s) => s.setDialogOpen);
  const busy = useUpdateStore((s) => s.status === "downloading" || s.status === "installing");
  return (
    <Dialog open={open} onOpenChange={(o) => !busy && setOpen(o)}>
      <DialogContent className="flex max-h-[80vh] flex-col gap-3 sm:max-w-lg">
        {open && <Body onClose={() => setOpen(false)} />}
      </DialogContent>
    </Dialog>
  );
}

function Body({ onClose }: { onClose: () => void }) {
  const { update, status, downloaded, total, error, install, skip } = useUpdateStore();
  const current = useQuery({ queryKey: ["app-info"], queryFn: api.appInfo }).data?.version;
  const running = useTerminalStore(
    (s) => s.tabs.filter((t) => t.status === "open" || t.status === "connecting").length,
  );
  const busy = status === "downloading" || status === "installing";

  if (!update) {
    return (
      <DialogHeader>
        <DialogTitle>Updates</DialogTitle>
        <DialogDescription>RidgeSSH is up to date.</DialogDescription>
      </DialogHeader>
    );
  }

  const pct = total ? Math.min(100, Math.round((downloaded / total) * 100)) : null;
  return (
    <>
      <DialogHeader>
        <DialogTitle>RidgeSSH {update.version}</DialogTitle>
        <DialogDescription>
          {current ? `You have ${current}. ` : ""}
          {update.date ? `Released ${update.date.slice(0, 10)}.` : ""}
        </DialogDescription>
      </DialogHeader>

      {update.body?.trim() && (
        <ScrollArea className="min-h-0 flex-1 rounded-md border">
          <pre className="max-h-72 p-3 font-sans text-sm whitespace-pre-wrap">
            {update.body.trim()}
          </pre>
        </ScrollArea>
      )}

      {running > 0 && !busy && (
        <p className="flex items-start gap-1.5 text-sm text-amber-700 dark:text-amber-400">
          <TriangleAlertIcon className="mt-0.5 size-4 shrink-0" />
          {running} open terminal session{running === 1 ? "" : "s"} will be closed when RidgeSSH
          restarts.
        </p>
      )}

      {busy && (
        <div className="grid gap-1.5" role="status">
          <div className="h-1.5 overflow-hidden rounded-full bg-muted">
            <div
              className="h-full bg-primary transition-[width]"
              style={{ width: `${status === "installing" ? 100 : (pct ?? 5)}%` }}
            />
          </div>
          <p className="text-xs text-muted-foreground tabular-nums">
            {status === "installing"
              ? "Installing… RidgeSSH will restart."
              : `Downloading ${mb(downloaded)}${total ? ` of ${mb(total)}` : ""}`}
          </p>
        </div>
      )}

      {status === "error" && error && <p className="text-sm text-destructive">{error}</p>}

      <p className="text-xs text-muted-foreground">
        Your hosts, settings and backups are kept. Your ~/.ssh/config is not touched.
      </p>

      <DialogFooter>
        <Button
          variant="ghost"
          className="sm:mr-auto"
          onClick={() => skip(update.version)}
          disabled={busy}
        >
          Skip this version
        </Button>
        <Button variant="outline" onClick={onClose} disabled={busy}>
          Later
        </Button>
        <Button onClick={() => void install()} disabled={busy}>
          {busy ? <LoaderCircleIcon className="animate-spin" /> : <DownloadIcon />}
          {busy ? "Updating…" : "Install and restart"}
        </Button>
      </DialogFooter>
    </>
  );
}
