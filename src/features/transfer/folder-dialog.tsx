import { useId, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FolderDownIcon, FolderOpenIcon } from "lucide-react";
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
import { useHostsData, useVariants, variantsFor } from "@/features/hosts/queries";
import { UserSelect } from "@/features/hosts/user-select";
import { useTerminalStore } from "@/features/terminal/store";
import { api, type FolderSpec, type Host } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";

/** Last path component of a remote path ("~" for the home folder). */
const remoteName = (p: string) => p.trim().replace(/\/+$/, "").split("/").pop() || p.trim();

export function FolderDialog() {
  const target = useUiStore((s) => s.folderTransfer);
  const close = useUiStore((s) => s.closeFolderTransfer);
  const host = useHostsData().find((h) => h.id === target?.hostId);
  return (
    <Dialog open={!!target && !!host} onOpenChange={(o) => !o && close()}>
      <DialogContent className="sm:max-w-xl">
        {target && host && (
          <Body key={host.id} host={host} initialUser={target.user ?? null} onDone={close} />
        )}
      </DialogContent>
    </Dialog>
  );
}

function Body({
  host,
  initialUser,
  onDone,
}: {
  host: Host;
  initialUser: string | null;
  onDone: () => void;
}) {
  const last = useQuery({
    queryKey: ["folder-last", host.id],
    queryFn: () => api.folderLast(host.id),
  }).data;
  const [user, setUser] = useState(initialUser);
  const [remote, setRemote] = useState<string | null>(null);
  const [local, setLocal] = useState<string | null>(null);
  const [unpack, setUnpack] = useState(false);
  const [keepZip, setKeepZip] = useState(true);
  const remoteId = useId();
  const localId = useId();
  const users = variantsFor(host, useVariants()).map((v) => v.user);

  const remoteValue = remote ?? last?.remote ?? "";
  const localValue = local ?? last?.local ?? "";
  const shown = user ? `${user}@${host.alias}` : host.alias;
  const ready = remoteValue.trim() !== "" && localValue !== "";

  const pickLocal = async () => {
    const dir = await openDialog({
      directory: true,
      title: "Save the folder to",
      defaultPath: localValue || undefined,
    });
    if (typeof dir === "string") setLocal(dir);
  };

  const start = () => {
    const spec: FolderSpec = {
      hostId: host.id,
      user,
      remotePath: remoteValue.trim(),
      localDir: localValue,
      unpack,
      keepZip: unpack ? keepZip : true,
    };
    useTerminalStore
      .getState()
      .openJob(host, { kind: "folder", spec }, `⇣ ${remoteName(spec.remotePath)} ← ${shown}`);
    onDone();
  };

  const steps = [
    "Check: the folder exists, its size, free space in /tmp, zip (or python3) available",
    "Zip on the server into a private temporary file in /tmp",
    "Download, then compare SHA-256 checksums",
    ...(unpack
      ? [
          `Unpack here${keepZip ? ", keeping the .zip" : " and delete the .zip"} (an existing folder is renamed to name.bak-<date> first)`,
        ]
      : []),
    "Clean up: delete the temporary archive on the server",
  ];

  return (
    <>
      <DialogHeader>
        <DialogTitle>Download folder — {shown}</DialogTitle>
        <DialogDescription>
          Zips the folder on the server and downloads one archive, which is much faster than copying
          many small files. Runs in a terminal tab, so you can answer prompts there.
        </DialogDescription>
      </DialogHeader>

      <div className="grid gap-4">
        <div className="grid gap-1.5">
          <Label htmlFor={remoteId}>Remote folder</Label>
          <Input
            id={remoteId}
            value={remoteValue}
            onChange={(e) => setRemote(e.target.value)}
            placeholder="/var/www/site or ~/app"
            className="font-mono"
            spellCheck={false}
            autoFocus
          />
        </div>
        <UserSelect
          users={users}
          value={user}
          onChange={setUser}
          defaultLabel={host.user ? `${host.user} (Host block)` : "Default (Host block)"}
        />
        <div className="grid gap-1.5">
          <Label htmlFor={localId}>Save to</Label>
          <div className="flex gap-2">
            <Input
              id={localId}
              value={localValue}
              readOnly
              placeholder="Choose a folder on this PC"
              className="font-mono text-xs"
            />
            <Button variant="outline" onClick={() => void pickLocal()}>
              <FolderOpenIcon /> Choose…
            </Button>
          </div>
        </div>
        <div className="grid gap-2">
          <label className="flex items-center gap-2 text-sm">
            <Checkbox checked={unpack} onCheckedChange={(v) => setUnpack(v === true)} />
            Unpack after downloading
          </label>
          {unpack && (
            <label className="flex items-center gap-2 pl-6 text-sm">
              <Checkbox checked={keepZip} onCheckedChange={(v) => setKeepZip(v === true)} />
              Keep the .zip as well
            </label>
          )}
        </div>
        <ol
          aria-label="What will happen"
          className="grid list-decimal gap-0.5 rounded-md border bg-muted/30 py-2 pr-3 pl-8 text-xs text-muted-foreground"
        >
          {steps.map((s) => (
            <li key={s}>{s}</li>
          ))}
        </ol>
      </div>

      <DialogFooter>
        <Button variant="outline" onClick={onDone}>
          Cancel
        </Button>
        <Button disabled={!ready} onClick={start}>
          <FolderDownIcon /> Download
        </Button>
      </DialogFooter>
    </>
  );
}
