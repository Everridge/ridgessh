import { useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  ArchiveIcon,
  FileUpIcon,
  FolderOpenIcon,
  LoaderCircleIcon,
  RotateCcwIcon,
  Trash2Icon,
  TriangleAlertIcon,
} from "lucide-react";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { Badge } from "@/components/ui/badge";
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
import { ScrollArea } from "@/components/ui/scroll-area";
import { formatTs } from "@/features/hosts/format";
import type { BackupInfo } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { usePasswordFields } from "./password-fields";
import {
  formatSize,
  reasonLabel,
  useBackupStatus,
  useBackups,
  useCreateBackup,
  useDeleteBackup,
  useRememberPassword,
} from "./queries";

export function BackupsDialog() {
  const open = useUiStore((s) => s.backupsOpen);
  const setOpen = useUiStore((s) => s.setBackupsOpen);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="flex h-[80vh] flex-col gap-3 sm:max-w-3xl">
        {open && <Body />}
      </DialogContent>
    </Dialog>
  );
}

function Body() {
  const status = useBackupStatus().data;
  const backups = useBackups();
  const create = useCreateBackup();
  const del = useDeleteBackup();
  const [deleting, setDeleting] = useState<BackupInfo | null>(null);
  const [asking, setAsking] = useState(false);
  const ui = useUiStore.getState;

  const backUpNow = () => (status?.passwordRemembered ? create.mutate(undefined) : setAsking(true));
  const restoreFromFile = async () => {
    const path = await openDialog({
      title: "Restore a RidgeSSH backup",
      filters: [{ name: "RidgeSSH backup", extensions: ["rsbak"] }],
    });
    if (typeof path === "string")
      ui().setRestoreTarget({ path, name: path.split(/[\\/]/).pop() ?? path });
  };
  const openSettings = () => {
    ui().setBackupsOpen(false);
    ui().setSettingsOpen(true);
  };

  return (
    <>
      <DialogHeader>
        <DialogTitle>Backups</DialogTitle>
        <DialogDescription className="flex items-center gap-1">
          Encrypted backups of your hosts, groups, notes, history and SSH config in{" "}
          <code className="truncate">{status?.backupDir}</code>
          {status && (
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label="Show in Explorer"
              onClick={() => void revealItemInDir(status.backupDir)}
            >
              <FolderOpenIcon />
            </Button>
          )}
        </DialogDescription>
      </DialogHeader>

      {status && !status.passwordRemembered && (
        <Banner tone="warn">
          Automatic backups are off because no backup password is remembered.
          <Button size="xs" variant="outline" className="ml-auto" onClick={openSettings}>
            Set password
          </Button>
        </Banner>
      )}
      {status?.passwordRemembered && !status.autoBackup && (
        <Banner tone="muted">
          Automatic backups are turned off in Settings.
          <Button size="xs" variant="outline" className="ml-auto" onClick={openSettings}>
            Settings
          </Button>
        </Banner>
      )}
      {status?.lastError && (
        <Banner tone="error">The last automatic backup failed: {status.lastError}</Banner>
      )}

      <div className="flex gap-2">
        <Button onClick={backUpNow} disabled={create.isPending}>
          {create.isPending ? <LoaderCircleIcon className="animate-spin" /> : <ArchiveIcon />}
          Back up now
        </Button>
        <Button variant="outline" onClick={() => void restoreFromFile()}>
          <FileUpIcon /> Restore from file…
        </Button>
      </div>

      <ScrollArea className="min-h-0 flex-1 rounded-md border">
        {backups.data?.length ? (
          <ul className="divide-y" aria-label="Backup files">
            {backups.data.map((b) => (
              <li key={b.fileName} className="flex items-center gap-3 px-3 py-2 text-sm">
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="font-medium">{formatTs(b.createdAt)}</span>
                    {b.manual && (
                      <Badge variant="secondary" title="Never deleted automatically">
                        kept
                      </Badge>
                    )}
                  </div>
                  <div className="truncate text-xs text-muted-foreground">
                    {reasonLabel(b.reason)} · {formatSize(b.size)}
                  </div>
                </div>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => ui().setRestoreTarget({ path: b.path, name: b.fileName })}
                >
                  <RotateCcwIcon /> Restore…
                </Button>
                <Button
                  size="icon-sm"
                  variant="ghost"
                  aria-label={`Show ${b.fileName} in Explorer`}
                  onClick={() => void revealItemInDir(b.path)}
                >
                  <FolderOpenIcon />
                </Button>
                <Button
                  size="icon-sm"
                  variant="ghost"
                  aria-label={`Delete ${b.fileName}`}
                  onClick={() => setDeleting(b)}
                >
                  <Trash2Icon />
                </Button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="p-6 text-center text-sm text-muted-foreground">
            {backups.isPending ? "Loading…" : "No backups yet."}
          </p>
        )}
      </ScrollArea>

      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(o) => !o && setDeleting(null)}
        title="Delete this backup?"
        description={`${deleting?.fileName} is deleted from the backup folder. This cannot be undone.`}
        confirmLabel="Delete"
        destructive
        onConfirm={() => {
          if (deleting) del.mutate(deleting.fileName);
          setDeleting(null);
        }}
      />
      {asking && <OneTimePassword onClose={() => setAsking(false)} />}
    </>
  );
}

function Banner({
  tone,
  children,
}: {
  tone: "warn" | "error" | "muted";
  children: React.ReactNode;
}) {
  const cls = {
    warn: "border-amber-500/40 bg-amber-500/10 text-amber-800 dark:text-amber-300",
    error: "border-destructive/40 bg-destructive/10 text-destructive",
    muted: "text-muted-foreground",
  }[tone];
  return (
    <div
      role="status"
      className={`flex items-center gap-2 rounded-md border px-3 py-2 text-sm ${cls}`}
    >
      <TriangleAlertIcon className="size-4 shrink-0" />
      {children}
    </div>
  );
}

/** Password for a manual backup when none is remembered (optionally remembering it). */
function OneTimePassword({ onClose }: { onClose: () => void }) {
  const { fields, password } = usePasswordFields();
  const [rememberIt, setRememberIt] = useState(true);
  const create = useCreateBackup();
  const remember = useRememberPassword();
  const busy = create.isPending || remember.isPending;

  const submit = async () => {
    if (!password) return;
    if (rememberIt) await remember.mutateAsync(password);
    create.mutate(password, { onSuccess: onClose });
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Backup password</DialogTitle>
          <DialogDescription>
            You need this password to restore the backup. It cannot be recovered if you lose it.
          </DialogDescription>
        </DialogHeader>
        {fields}
        <label className="flex items-center gap-2 text-sm">
          <Checkbox checked={rememberIt} onCheckedChange={(v) => setRememberIt(v === true)} />
          Remember in Windows Credential Manager (turns on automatic backups)
        </label>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button disabled={!password || busy} onClick={() => void submit()}>
            {busy && <LoaderCircleIcon className="animate-spin" />}
            Back up
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
