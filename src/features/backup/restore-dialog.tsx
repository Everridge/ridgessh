import { useId, useState } from "react";
import { LoaderCircleIcon, LockOpenIcon, RotateCcwIcon } from "lucide-react";
import { toast } from "sonner";
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
import { ScrollArea } from "@/components/ui/scroll-area";
import { DiffView } from "@/features/config-write/diff-view";
import { formatTs } from "@/features/hosts/format";
import { errorMessage, type BackupInspection } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { reasonLabel, useInspectBackup, useRestoreBackup } from "./queries";

export function RestoreDialog() {
  const target = useUiStore((s) => s.restoreTarget);
  const setTarget = useUiStore((s) => s.setRestoreTarget);
  return (
    <Dialog open={target !== null} onOpenChange={(o) => !o && setTarget(null)}>
      <DialogContent className="flex max-h-[88vh] flex-col gap-3 sm:max-w-4xl">
        {target && (
          <Body
            key={target.path}
            path={target.path}
            name={target.name}
            onDone={() => setTarget(null)}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

function Body({ path, name, onDone }: { path: string; name: string; onDone: () => void }) {
  const [password, setPassword] = useState("");
  const inspect = useInspectBackup();
  const id = useId();
  const result = inspect.data;

  return (
    <>
      <DialogHeader>
        <DialogTitle>Restore backup</DialogTitle>
        <DialogDescription className="truncate">{name}</DialogDescription>
      </DialogHeader>
      {result ? (
        <Preview inspection={result} password={password} onDone={onDone} />
      ) : (
        <form
          className="grid gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            inspect.mutate({ path, password });
          }}
        >
          <div className="grid max-w-sm gap-1.5">
            <Label htmlFor={id}>Password of this backup</Label>
            <Input
              id={id}
              type="password"
              autoFocus
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
            {inspect.isError && (
              <p className="text-sm text-destructive">{errorMessage(inspect.error)}</p>
            )}
          </div>
          <p className="text-sm text-muted-foreground">
            The backup is decrypted to show what would change. Nothing is restored yet.
          </p>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onDone}>
              Cancel
            </Button>
            <Button type="submit" disabled={!password || inspect.isPending}>
              {inspect.isPending ? <LoaderCircleIcon className="animate-spin" /> : <LockOpenIcon />}
              Open backup
            </Button>
          </DialogFooter>
        </form>
      )}
    </>
  );
}

function Preview({
  inspection: i,
  password,
  onDone,
}: {
  inspection: BackupInspection;
  password: string;
  onDone: () => void;
}) {
  const configDiffers = !!i.config && i.config.additions + i.config.deletions > 0;
  const [restoreDb, setRestoreDb] = useState(true);
  const [restoreConfig, setRestoreConfig] = useState(false);
  const [word, setWord] = useState("");
  const restore = useRestoreBackup();
  const wordId = useId();
  const h = i.hosts;
  const ready =
    (restoreDb || restoreConfig) &&
    (!restoreConfig || word === i.confirmationWord) &&
    !restore.isPending;

  const run = () =>
    restore.mutate(
      {
        path: i.path,
        password,
        options: {
          restoreDb,
          restoreConfig,
          configBaseHash: restoreConfig ? (i.config?.baseHash ?? null) : null,
          confirmation: restoreConfig ? word : null,
        },
      },
      {
        onSuccess: (r) => {
          toast.success(`Restored the backup from ${formatTs(i.manifest.createdAt)}`, {
            description: `The state before restoring was saved as ${r.safetyBackup.fileName}`,
          });
          onDone();
        },
        onError: (e) => toast.error(errorMessage(e)),
      },
    );

  return (
    <>
      <p className="text-sm text-muted-foreground">
        Made {formatTs(i.manifest.createdAt)} ({reasonLabel(i.manifest.reason).toLowerCase()}) by
        RidgeSSH {i.manifest.appVersion}: {h.total} hosts, {i.manifest.groupCount} groups.
      </p>
      <ScrollArea className="min-h-0 flex-1">
        <div className="grid gap-4 pr-3">
          <section className="grid gap-2 rounded-md border p-3">
            <label className="flex items-center gap-2 text-sm font-medium">
              <Checkbox checked={restoreDb} onCheckedChange={(v) => setRestoreDb(v === true)} />
              Restore hosts, groups, notes, history and settings
            </label>
            <div className="grid gap-2 pl-6 text-sm sm:grid-cols-3">
              <HostList title="Come back" names={h.added} />
              <HostList title="Removed (not in the backup)" names={h.removed} />
              <HostList title="Changed back" names={h.changed} />
            </div>
          </section>

          <section className="grid gap-2 rounded-md border p-3">
            <label className="flex items-center gap-2 text-sm font-medium">
              <Checkbox
                checked={restoreConfig}
                disabled={!configDiffers}
                onCheckedChange={(v) => setRestoreConfig(v === true)}
              />
              Restore <code>{i.configPath}</code>
            </label>
            {!i.config ? (
              <p className="pl-6 text-sm text-muted-foreground">This backup has no SSH config.</p>
            ) : !configDiffers ? (
              <p className="pl-6 text-sm text-muted-foreground">Same as your current config.</p>
            ) : (
              <div className="grid gap-1 pl-6">
                <p className="text-xs text-muted-foreground">
                  <span className="text-emerald-700 dark:text-emerald-400">
                    +{i.config.additions}
                  </span>{" "}
                  <span className="text-red-700 dark:text-red-400">−{i.config.deletions}</span>{" "}
                  lines compared with the current file
                </p>
                <div className="max-h-64 overflow-auto rounded-md border">
                  <DiffView lines={i.config.diff} />
                </div>
              </div>
            )}
          </section>
        </div>
      </ScrollArea>

      <DialogFooter className="items-end gap-3 sm:justify-between">
        <div className="grid w-full max-w-sm gap-1.5">
          {restoreConfig ? (
            <>
              <Label htmlFor={wordId} className="text-sm font-normal">
                Type <code className="font-semibold">{i.confirmationWord}</code> to replace the SSH
                config
              </Label>
              <Input
                id={wordId}
                value={word}
                onChange={(e) => setWord(e.target.value)}
                autoComplete="off"
                spellCheck={false}
              />
            </>
          ) : (
            <p className="text-xs text-muted-foreground">
              A backup of the current state is made before restoring.
            </p>
          )}
        </div>
        <div className="flex gap-2">
          <Button variant="outline" onClick={onDone}>
            Cancel
          </Button>
          <Button variant="destructive" disabled={!ready} onClick={run}>
            {restore.isPending ? <LoaderCircleIcon className="animate-spin" /> : <RotateCcwIcon />}
            Restore
          </Button>
        </div>
      </DialogFooter>
    </>
  );
}

function HostList({ title, names }: { title: string; names: string[] }) {
  return (
    <div className="min-w-0">
      <div className="text-xs text-muted-foreground">
        {title}: <span className="tabular-nums">{names.length}</span>
      </div>
      {names.length > 0 && (
        <ul className="mt-1 max-h-28 overflow-auto text-xs">
          {names.map((n) => (
            <li key={n} className="truncate font-mono">
              {n}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
