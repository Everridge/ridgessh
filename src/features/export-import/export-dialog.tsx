import { useMemo, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { DownloadIcon, KeyRoundIcon, TriangleAlertIcon } from "lucide-react";
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
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { usePasswordFields } from "@/features/backup/password-fields";
import { groupPath } from "@/features/hosts/filter";
import { useGroupsData, useHostsData } from "@/features/hosts/queries";
import { useScripts } from "@/features/scripts/queries";
import { api, errorMessage, type Host } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";

type Format = "encrypted" | "config";

const today = () => new Date().toISOString().slice(0, 10).replaceAll("-", "");

export function ExportDialog() {
  const target = useUiStore((s) => s.exportTarget);
  const close = useUiStore((s) => s.closeExport);
  return (
    <Dialog open={!!target} onOpenChange={(o) => !o && close()}>
      <DialogContent className="sm:max-w-2xl">
        {target && <Body initial={target.hostIds} onDone={close} />}
      </DialogContent>
    </Dialog>
  );
}

/** Toggles every id in `ids` on (if any is off) or off. */
function toggleAll(current: Set<string>, ids: string[]): Set<string> {
  const next = new Set(current);
  const allOn = ids.every((id) => next.has(id));
  for (const id of ids) {
    if (allOn) next.delete(id);
    else next.add(id);
  }
  return next;
}

function Body({ initial, onDone }: { initial: string[] | null; onDone: () => void }) {
  const allHosts = useHostsData();
  const groups = useGroupsData();
  const scripts = useScripts().data ?? [];
  const live = useMemo(() => allHosts.filter((h) => !h.deletedAt), [allHosts]);

  const [format, setFormat] = useState<Format>("encrypted");
  const [hostIds, setHostIds] = useState(() => new Set(initial ?? live.map((h) => h.id)));
  // Scripts come along when exporting everything; a focused export starts without them.
  const [scriptIds, setScriptIds] = useState<Set<string> | null>(null);
  const chosenScripts = scriptIds ?? new Set(initial ? [] : scripts.map((s) => s.id));
  const [includeKeys, setIncludeKeys] = useState(false);
  const pw = usePasswordFields("Export password");

  // Hosts by group path, groups in tree order (top level first).
  const sections = useMemo(() => {
    const by = new Map<string, Host[]>();
    for (const h of live) {
      const path = groupPath(groups, h.groupId);
      by.set(path, [...(by.get(path) ?? []), h]);
    }
    return [...by.entries()].sort(([a], [b]) =>
      a === "" ? -1 : b === "" ? 1 : a.localeCompare(b),
    );
  }, [live, groups]);

  const selectedHosts = live.filter((h) => hostIds.has(h.id));
  const withKeys = selectedHosts.filter((h) => h.identityFile).length;
  const encrypted = format === "encrypted";
  const nothing = selectedHosts.length === 0 && (!encrypted || chosenScripts.size === 0);

  const run = useMutation({
    mutationFn: async () => {
      const ids = selectedHosts.map((h) => h.id);
      const path = await saveDialog({
        title: encrypted ? "Save export" : "Save ssh config",
        defaultPath: encrypted ? `ridgessh-export-${today()}.rsexport` : "ssh_config",
        filters: encrypted ? [{ name: "RidgeSSH export", extensions: ["rsexport"] }] : undefined,
      });
      if (!path) return null;
      if (!encrypted) return { path: await api.exportConfigFile(path, ids), summary: null };
      const summary = await api.exportCreate(path, pw.password!, {
        hostIds: ids,
        scriptIds: scripts.filter((s) => chosenScripts.has(s.id)).map((s) => s.id),
        includeKeys,
      });
      return { path: summary.path, summary };
    },
    onSuccess: (r) => {
      if (!r) return;
      const s = r.summary;
      const what = s
        ? [
            `${s.hosts} host${s.hosts === 1 ? "" : "s"}`,
            s.scripts && `${s.scripts} script${s.scripts === 1 ? "" : "s"}`,
            s.keys && `${s.keys} key file${s.keys === 1 ? "" : "s"}`,
          ]
            .filter(Boolean)
            .join(", ")
        : `${selectedHosts.length} host${selectedHosts.length === 1 ? "" : "s"}`;
      toast.success(`Exported ${what}`, {
        description: r.path,
        action: { label: "Show", onClick: () => void revealItemInDir(r.path) },
      });
      for (const w of s?.warnings ?? []) toast.warning(w);
      onDone();
    },
    onError: (e) => toast.error(errorMessage(e)),
  });

  return (
    <>
      <DialogHeader>
        <DialogTitle>Export</DialogTitle>
        <DialogDescription>
          Save hosts to a file to move them to another machine or share them.
        </DialogDescription>
      </DialogHeader>

      <Tabs value={format} onValueChange={(v) => setFormat(v as Format)}>
        <TabsList>
          <TabsTrigger value="encrypted">Encrypted RidgeSSH file</TabsTrigger>
          <TabsTrigger value="config">Plain ssh config</TabsTrigger>
        </TabsList>
      </Tabs>

      <div className="grid gap-1.5">
        <div className="flex items-center gap-2 text-sm">
          <Checkbox
            aria-label="All hosts"
            checked={
              hostIds.size === 0 ? false : selectedHosts.length === live.length || "indeterminate"
            }
            onCheckedChange={() =>
              setHostIds(
                toggleAll(
                  hostIds,
                  live.map((h) => h.id),
                ),
              )
            }
          />
          <span className="font-medium">
            Hosts ({selectedHosts.length} of {live.length})
          </span>
        </div>
        <div
          className="grid max-h-56 content-start gap-0.5 overflow-y-auto rounded-md border p-2 text-sm"
          aria-label="Hosts to export"
        >
          {sections.map(([path, hs]) => (
            <div key={path} className="grid gap-0.5">
              {path && (
                <label className="flex items-center gap-2 pt-1 text-xs font-medium text-muted-foreground">
                  <Checkbox
                    checked={
                      hs.every((h) => hostIds.has(h.id))
                        ? true
                        : hs.some((h) => hostIds.has(h.id)) && "indeterminate"
                    }
                    onCheckedChange={() =>
                      setHostIds(
                        toggleAll(
                          hostIds,
                          hs.map((h) => h.id),
                        ),
                      )
                    }
                  />
                  {path}
                </label>
              )}
              {hs.map((h) => (
                <label
                  key={h.id}
                  className={path ? "flex items-center gap-2 pl-5" : "flex items-center gap-2"}
                >
                  <Checkbox
                    checked={hostIds.has(h.id)}
                    onCheckedChange={() => setHostIds(toggleAll(hostIds, [h.id]))}
                  />
                  <span className="font-mono text-xs">{h.alias}</span>
                  <span className="truncate text-xs text-muted-foreground">
                    {[h.user, h.hostname].filter(Boolean).join("@")}
                  </span>
                </label>
              ))}
            </div>
          ))}
        </div>
      </div>

      {encrypted ? (
        <>
          {scripts.length > 0 && (
            <div className="grid gap-1.5 text-sm">
              <span className="font-medium">Scripts</span>
              <div className="flex flex-wrap gap-x-4 gap-y-1">
                {scripts.map((s) => (
                  <label key={s.id} className="flex items-center gap-2">
                    <Checkbox
                      checked={chosenScripts.has(s.id)}
                      onCheckedChange={() => setScriptIds(toggleAll(chosenScripts, [s.id]))}
                    />
                    {s.name}
                  </label>
                ))}
              </div>
            </div>
          )}

          <div className="grid gap-1">
            <label className="flex items-center gap-2 text-sm">
              <Checkbox
                checked={includeKeys}
                onCheckedChange={(v) => setIncludeKeys(v === true)}
                disabled={withKeys === 0}
              />
              <KeyRoundIcon className="size-3.5" />
              Include private key files ({withKeys} host{withKeys === 1 ? "" : "s"} name one)
            </label>
            {includeKeys && (
              <p className="flex items-start gap-1.5 pl-6 text-xs text-amber-600 dark:text-amber-400">
                <TriangleAlertIcon className="mt-0.5 size-3.5 shrink-0" />
                Anyone with the file and its password can log in as you. Only the files named in
                IdentityFile are included.
              </p>
            )}
          </div>

          {pw.fields}
          <p className="-mt-1 text-xs text-muted-foreground">
            The password is not stored. You need it to import the file.
          </p>
        </>
      ) : (
        <p className="flex items-start gap-1.5 rounded-md border border-amber-500/40 bg-amber-500/5 p-2 text-xs">
          <TriangleAlertIcon className="mt-0.5 size-3.5 shrink-0 text-amber-500" />
          An unencrypted text file with Host blocks for the selected hosts: connection settings
          only, without descriptions, notes, groups or scripts.
        </p>
      )}

      <DialogFooter>
        <Button variant="outline" onClick={onDone}>
          Cancel
        </Button>
        <Button
          disabled={nothing || (encrypted && !pw.password) || run.isPending}
          onClick={() => run.mutate()}
        >
          <DownloadIcon /> {run.isPending ? "Exporting…" : "Export…"}
        </Button>
      </DialogFooter>
    </>
  );
}
