import { useId, useMemo, useState } from "react";
import { FileWarningIcon, LoaderCircleIcon, ShieldCheckIcon } from "lucide-react";
import { toast } from "sonner";
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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useHostsData } from "@/features/hosts/queries";
import { errorMessage, isAppError, type ConfigCandidate, type ConfigChange } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/ui";
import { DiffView } from "./diff-view";
import { opLabel } from "./pending";
import { useApplyConfig, useConfigCandidates, useConfigPreview } from "./queries";

const OP_BADGE: Record<ConfigCandidate["op"], string> = {
  update: "border-violet-500/40 text-violet-700 dark:text-violet-400",
  add: "border-emerald-500/40 text-emerald-700 dark:text-emerald-400",
  remove: "border-red-500/40 text-red-700 dark:text-red-400",
};

/**
 * The only place that writes ~/.ssh/config: pick changes, review the diff, type the
 * confirmation word. The backend re-checks the word and that nothing moved since the preview.
 */
export function ConfigWriteDialog() {
  const state = useUiStore((s) => s.configWrite);
  const close = useUiStore((s) => s.closeConfigWrite);
  return (
    <Dialog open={state !== null} onOpenChange={(o) => !o && close()}>
      <DialogContent className="flex h-[85vh] flex-col gap-3 sm:max-w-5xl">
        {state && <Body focus={state.focus} onDone={close} />}
      </DialogContent>
    </Dialog>
  );
}

function Body({ focus, onDone }: { focus: string[] | null; onDone: () => void }) {
  const candidatesQuery = useConfigCandidates();
  const candidates = useMemo(() => candidatesQuery.data ?? [], [candidatesQuery.data]);
  const hosts = useHostsData();
  const byId = useMemo(() => new Map(hosts.map((h) => [h.id, h])), [hosts]);
  // Only the user's toggles are stored; everything else follows the defaults.
  const [toggled, setToggled] = useState<Record<string, boolean>>({});
  const [word, setWord] = useState("");
  const wordId = useId();

  const isSelected = (c: ConfigCandidate) =>
    !c.blocked && (toggled[c.hostId] ?? (focus ? focus.includes(c.hostId) : c.selected));
  const changes: ConfigChange[] = candidates
    .filter(isSelected)
    .map(({ hostId, op }) => ({ hostId, op }));

  const preview = useConfigPreview(changes);
  const apply = useApplyConfig();
  const p = changes.length > 0 ? preview.data : undefined;
  const ready = !!p && !preview.isFetching && !preview.isError;
  const matches = !!p && word === p.confirmationWord;

  const write = () => {
    if (!p) return;
    apply.mutate(
      { changes, baseHash: p.baseHash, resultHash: p.resultHash, confirmation: word },
      {
        onSuccess: (r) => {
          toast.success(
            `Wrote ${r.changed} change${r.changed === 1 ? "" : "s"} to ${r.configPath}`,
            { description: r.backupPath ? `Previous version saved as ${r.backupPath}` : undefined },
          );
          onDone();
        },
        onError: (e) => {
          toast.error(errorMessage(e));
          if (isAppError(e) && e.kind === "conflict") void candidatesQuery.refetch();
        },
      },
    );
  };

  return (
    <>
      <DialogHeader>
        <DialogTitle>Write to SSH config</DialogTitle>
        <DialogDescription>
          Review exactly what changes in <code>{p?.configPath ?? "~/.ssh/config"}</code>. Only the
          lines shown change; a copy of the current file is saved to the backup folder first.
        </DialogDescription>
      </DialogHeader>

      {candidatesQuery.isPending ? (
        <Loading />
      ) : candidates.length === 0 ? (
        <p className="flex flex-1 items-center justify-center p-6 text-center text-sm text-muted-foreground">
          Nothing to write. When you edit the connection of a host from your config, add a
          RidgeSSH-only host, or trash a host that is in the config, it shows up here.
        </p>
      ) : (
        <div className="grid min-h-0 flex-1 grid-cols-[16rem_1fr] gap-3">
          <ScrollArea className="min-h-0 rounded-md border">
            <ul className="divide-y" aria-label="Changes">
              {candidates.map((c) => {
                const id = `cw-${c.hostId}`;
                return (
                  <li
                    key={c.hostId}
                    className={cn("grid gap-1 px-3 py-2", c.blocked && "opacity-70")}
                  >
                    <div className="flex items-center gap-2">
                      <Checkbox
                        id={id}
                        checked={isSelected(c)}
                        disabled={!!c.blocked}
                        onCheckedChange={(v) =>
                          setToggled((t) => ({ ...t, [c.hostId]: v === true }))
                        }
                      />
                      <Label htmlFor={id} className="min-w-0 flex-1 truncate font-medium">
                        {c.alias}
                      </Label>
                      <Badge variant="outline" className={OP_BADGE[c.op]}>
                        {opLabel(c.op, byId.get(c.hostId))}
                      </Badge>
                    </div>
                    {c.blocked && (
                      <p className="pl-6 text-xs text-amber-700 dark:text-amber-400">{c.blocked}</p>
                    )}
                  </li>
                );
              })}
            </ul>
          </ScrollArea>

          <div className="flex min-h-0 flex-col gap-2">
            <div className="flex h-5 items-center gap-2 text-xs text-muted-foreground">
              {p && (
                <>
                  <span className="text-emerald-700 tabular-nums dark:text-emerald-400">
                    +{p.additions}
                  </span>
                  <span className="text-red-700 tabular-nums dark:text-red-400">
                    −{p.deletions}
                  </span>
                  <span>lines</span>
                  {!p.fileExists && <span>· the file will be created</span>}
                </>
              )}
              {preview.isFetching && <LoaderCircleIcon className="size-3.5 animate-spin" />}
            </div>
            <ScrollArea className="min-h-0 flex-1 rounded-md border">
              {changes.length === 0 ? (
                <p className="p-6 text-center text-sm text-muted-foreground">
                  Select at least one change.
                </p>
              ) : preview.isError ? (
                <p className="flex gap-2 p-4 text-sm text-destructive">
                  <FileWarningIcon className="size-4 shrink-0" />
                  {errorMessage(preview.error)}
                </p>
              ) : p ? (
                <DiffView lines={p.diff} />
              ) : (
                <Loading />
              )}
            </ScrollArea>
          </div>
        </div>
      )}

      <DialogFooter className="items-end gap-3 sm:justify-between">
        <div className="grid w-full max-w-sm gap-1.5">
          <Label htmlFor={wordId} className="text-sm font-normal">
            Type{" "}
            <code className="font-semibold">{p?.confirmationWord ?? "the confirmation word"}</code>{" "}
            to write the file
          </Label>
          <Input
            id={wordId}
            value={word}
            onChange={(e) => setWord(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && matches && ready && write()}
            autoComplete="off"
            spellCheck={false}
            disabled={!p}
          />
        </div>
        <div className="flex gap-2">
          <Button variant="outline" onClick={onDone}>
            Cancel
          </Button>
          <Button disabled={!ready || !matches || apply.isPending} onClick={write}>
            {apply.isPending ? <LoaderCircleIcon className="animate-spin" /> : <ShieldCheckIcon />}
            Write config
          </Button>
        </div>
      </DialogFooter>
    </>
  );
}

function Loading() {
  return (
    <div className="flex flex-1 items-center justify-center p-6 text-muted-foreground">
      <LoaderCircleIcon className="size-5 animate-spin" />
    </div>
  );
}
