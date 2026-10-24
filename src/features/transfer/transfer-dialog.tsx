import { useEffect, useId, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FileIcon, FolderIcon, FolderOpenIcon, UploadIcon, XIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useHostsData } from "@/features/hosts/queries";
import { useTerminalStore } from "@/features/terminal/store";
import { api, type Host, type TransferSpec } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/ui";

const baseName = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;

export function TransferDialog() {
  const target = useUiStore((s) => s.transfer);
  const close = useUiStore((s) => s.closeTransfer);
  const host = useHostsData().find((h) => h.id === target?.hostId);
  return (
    <Dialog open={!!target && !!host} onOpenChange={(o) => !o && close()}>
      <DialogContent className="sm:max-w-xl">
        {target && host && (
          <Body key={host.id} host={host} initial={target.direction} onDone={close} />
        )}
      </DialogContent>
    </Dialog>
  );
}

function Body({
  host,
  initial,
  onDone,
}: {
  host: Host;
  initial: "upload" | "download";
  onDone: () => void;
}) {
  const [direction, setDirection] = useState(initial);
  const lastRemote = useQuery({
    queryKey: ["transfer-last-remote", host.id],
    queryFn: () => api.transferLastRemote(host.id),
  });
  const [remote, setRemote] = useState<string | null>(null);
  const [files, setFiles] = useState<string[]>([]);
  const [localDir, setLocalDir] = useState("");
  const [recursive, setRecursive] = useState(false);
  const [dragging, setDragging] = useState(false);
  const remoteId = useId();
  const remoteValue = remote ?? lastRemote.data ?? "";

  const add = (paths: string[]) => setFiles((f) => [...f, ...paths.filter((p) => !f.includes(p))]);

  // Files dropped onto the window while uploading (Tauri delivers real paths).
  useEffect(() => {
    if (direction !== "upload") return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    try {
      void getCurrentWebview()
        .onDragDropEvent((e) => {
          if (e.payload.type === "enter" || e.payload.type === "over") setDragging(true);
          else if (e.payload.type === "leave") setDragging(false);
          else {
            setDragging(false);
            add(e.payload.paths);
          }
        })
        .then((u) => (cancelled ? u() : (unlisten = u)))
        .catch(() => {});
    } catch {
      // Not running inside Tauri (tests).
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [direction]);

  const pick = async (directory: boolean) => {
    const picked = await openDialog({ multiple: !directory, directory });
    if (typeof picked === "string") add([picked]);
    else if (Array.isArray(picked)) add(picked);
  };
  const pickLocalDir = async () => {
    const dir = await openDialog({ directory: true, title: "Save downloads to" });
    if (typeof dir === "string") setLocalDir(dir);
  };

  const spec: TransferSpec =
    direction === "upload"
      ? { hostId: host.id, direction, local: files, remote: remoteValue, recursive: false }
      : { hostId: host.id, direction, local: [localDir], remote: remoteValue, recursive };
  const ready =
    remoteValue.trim() !== "" && (direction === "upload" ? files.length > 0 : localDir !== "");

  const start = () => {
    const title =
      direction === "upload"
        ? `↑ ${files.length === 1 ? baseName(files[0]) : `${files.length} items`} → ${host.alias}`
        : `↓ ${baseName(remoteValue)} ← ${host.alias}`;
    useTerminalStore.getState().openJob(host, { kind: "transfer", spec }, title);
    onDone();
  };

  return (
    <>
      <DialogHeader>
        <DialogTitle>Copy files — {host.alias}</DialogTitle>
        <DialogDescription>
          Runs scp in a terminal tab, so you can answer password or host-key prompts there.
        </DialogDescription>
      </DialogHeader>
      <Tabs value={direction} onValueChange={(v) => setDirection(v as typeof direction)}>
        <TabsList>
          <TabsTrigger value="upload">Upload</TabsTrigger>
          <TabsTrigger value="download">Download</TabsTrigger>
        </TabsList>
      </Tabs>

      {direction === "upload" ? (
        <div className="grid gap-2">
          <div
            className={cn(
              "grid min-h-24 content-start gap-1 rounded-md border border-dashed p-2 text-sm",
              dragging && "border-primary bg-primary/5",
            )}
            aria-label="Files to upload"
          >
            {files.length === 0 ? (
              <p className="p-4 text-center text-muted-foreground">
                Drop files or folders here, or add them below.
              </p>
            ) : (
              files.map((f) => (
                <div key={f} className="flex items-center gap-2">
                  <FileIcon className="size-3.5 shrink-0 text-muted-foreground" />
                  <span className="min-w-0 flex-1 truncate font-mono text-xs" title={f}>
                    {f}
                  </span>
                  <Button
                    size="icon-xs"
                    variant="ghost"
                    aria-label={`Remove ${baseName(f)}`}
                    onClick={() => setFiles((x) => x.filter((p) => p !== f))}
                  >
                    <XIcon />
                  </Button>
                </div>
              ))
            )}
          </div>
          <div className="flex gap-2">
            <Button size="sm" variant="outline" onClick={() => void pick(false)}>
              <FileIcon /> Add files…
            </Button>
            <Button size="sm" variant="outline" onClick={() => void pick(true)}>
              <FolderIcon /> Add folder…
            </Button>
          </div>
        </div>
      ) : (
        <div className="grid gap-2">
          <Label>Save to</Label>
          <div className="flex gap-2">
            <Input
              value={localDir}
              readOnly
              placeholder="Choose a local folder"
              aria-label="Local folder"
              className="font-mono text-xs"
            />
            <Button variant="outline" onClick={() => void pickLocalDir()}>
              <FolderOpenIcon /> Choose…
            </Button>
          </div>
          <label className="flex items-center gap-2 text-sm">
            <Checkbox checked={recursive} onCheckedChange={(v) => setRecursive(v === true)} />
            The remote path is a folder (copy everything in it)
          </label>
        </div>
      )}

      <div className="grid gap-1.5">
        <Label htmlFor={remoteId}>
          {direction === "upload" ? "Remote folder" : "Remote file or folder"}
        </Label>
        <Input
          id={remoteId}
          value={remoteValue}
          onChange={(e) => setRemote(e.target.value)}
          placeholder={
            direction === "upload" ? "/tmp or uploads (relative to home)" : "/var/log/syslog"
          }
          className="font-mono"
          spellCheck={false}
        />
      </div>

      <DialogFooter>
        <Button variant="outline" onClick={onDone}>
          Cancel
        </Button>
        <Button disabled={!ready} onClick={start}>
          <UploadIcon className={cn(direction === "download" && "rotate-180")} />
          {direction === "upload" ? "Upload" : "Download"}
        </Button>
      </DialogFooter>
    </>
  );
}
