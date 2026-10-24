import { useId, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FileUpIcon, FolderOpenIcon, TriangleAlertIcon } from "lucide-react";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { GroupSelect } from "@/features/groups/group-select";
import { invalidateAll } from "@/features/hosts/queries";
import {
  api,
  errorMessage,
  type ImportPreview,
  type ImportResult,
  type ImportSource,
  type ImportStrategy,
  type ItemStatus,
} from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { STATUS_LABELS, STRATEGY_LABELS, strategiesFor, summarize } from "./import-plan";

export function ImportDialog() {
  const open = useUiStore((s) => s.importOpen);
  const setOpen = useUiStore((s) => s.setImportOpen);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="sm:max-w-3xl">
        {open && <Body onDone={() => setOpen(false)} />}
      </DialogContent>
    </Dialog>
  );
}

const isExport = (path: string) => /\.rsexport$/i.test(path);
const baseName = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;

function Body({ onDone }: { onDone: () => void }) {
  const [path, setPath] = useState<string | null>(null);
  const [password, setPassword] = useState("");
  const [loaded, setLoaded] = useState<{ source: ImportSource; preview: ImportPreview } | null>(
    null,
  );
  const pwId = useId();

  const inspect = useMutation({
    mutationFn: async (source: ImportSource) => ({
      source,
      preview: await api.importPreview(source),
    }),
    onSuccess: setLoaded,
    onError: (e) => toast.error(errorMessage(e)),
  });

  const pick = async () => {
    const picked = await openDialog({
      title: "Import from",
      filters: [{ name: "RidgeSSH export or ssh config", extensions: ["rsexport", "*"] }],
    });
    if (typeof picked !== "string") return;
    setPath(picked);
    setLoaded(null);
    // A plain config needs no password: show its preview straight away.
    if (!isExport(picked)) inspect.mutate({ kind: "config", path: picked });
  };

  if (loaded) {
    return (
      <Review
        source={loaded.source}
        preview={loaded.preview}
        onBack={() => setLoaded(null)}
        onDone={onDone}
      />
    );
  }

  return (
    <>
      <DialogHeader>
        <DialogTitle>Import</DialogTitle>
        <DialogDescription>
          From a RidgeSSH export (.rsexport) or an ssh config file, e.g. from another machine. You
          review every host before anything is imported, and ~/.ssh/config is not changed.
        </DialogDescription>
      </DialogHeader>
      <div className="flex gap-2">
        <Input
          readOnly
          value={path ?? ""}
          placeholder="Choose a file"
          aria-label="Import file"
          className="font-mono text-xs"
        />
        <Button variant="outline" onClick={() => void pick()}>
          <FolderOpenIcon /> Choose…
        </Button>
      </div>
      {path && isExport(path) && (
        <form
          className="grid gap-1.5"
          onSubmit={(e) => {
            e.preventDefault();
            inspect.mutate({ kind: "export", path, password });
          }}
        >
          <Label htmlFor={pwId}>Export password</Label>
          <div className="flex gap-2">
            <Input
              id={pwId}
              type="password"
              autoFocus
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
            <Button type="submit" disabled={!password || inspect.isPending}>
              {inspect.isPending ? "Opening…" : "Open"}
            </Button>
          </div>
        </form>
      )}
      {path && !isExport(path) && inspect.isPending && (
        <p className="text-sm text-muted-foreground">Reading {baseName(path)}…</p>
      )}
      <DialogFooter>
        <Button variant="outline" onClick={onDone}>
          Cancel
        </Button>
      </DialogFooter>
    </>
  );
}

function StatusBadge({ status, differences }: { status: ItemStatus; differences: string[] }) {
  return (
    <Badge
      variant={status === "new" ? "default" : status === "differs" ? "outline" : "secondary"}
      title={differences.length ? `Differs in: ${differences.join(", ")}` : undefined}
    >
      {STATUS_LABELS[status]}
    </Badge>
  );
}

function StrategySelect({
  status,
  value,
  onChange,
  label,
  copyName,
}: {
  status: ItemStatus;
  value: ImportStrategy;
  onChange: (s: ImportStrategy) => void;
  label: string;
  copyName: string;
}) {
  return (
    <Select value={value} onValueChange={(v) => onChange(v as ImportStrategy)}>
      <SelectTrigger size="sm" className="w-40" aria-label={label}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {strategiesFor(status).map((s) => (
          <SelectItem key={s} value={s}>
            {s === "copy" ? `Copy as ${copyName}` : STRATEGY_LABELS[s]}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

function Review({
  source,
  preview,
  onBack,
  onDone,
}: {
  source: ImportSource;
  preview: ImportPreview;
  onBack: () => void;
  onDone: () => void;
}) {
  const qc = useQueryClient();
  const [hostChoice, setHostChoice] = useState<Record<string, ImportStrategy>>(() =>
    Object.fromEntries(preview.hosts.map((h) => [h.key, h.suggested])),
  );
  const [scriptChoice, setScriptChoice] = useState<Record<string, ImportStrategy>>(() =>
    Object.fromEntries(preview.scripts.map((s) => [s.key, s.suggested])),
  );
  const [targetGroupId, setTargetGroupId] = useState<string | null>(null);
  const newKeys = preview.keys.filter((k) => k.status === "new").length;
  const [installKeys, setInstallKeys] = useState(false);
  const groupId = useId();

  const existing = preview.hosts.filter((h) => h.status !== "new");
  const setExisting = (s: ImportStrategy) =>
    setHostChoice((c) => ({ ...c, ...Object.fromEntries(existing.map((h) => [h.key, s])) }));

  const plan = summarize(hostChoice, scriptChoice);

  const apply = useMutation({
    mutationFn: () =>
      api.importApply(source, {
        targetGroupId,
        hosts: Object.entries(hostChoice).map(([key, strategy]) => ({ key, strategy })),
        scripts: Object.entries(scriptChoice).map(([key, strategy]) => ({ key, strategy })),
        installKeys,
      }),
    onSuccess: (r) => {
      reportResult(r);
      onDone();
    },
    onError: (e) => toast.error(errorMessage(e)),
    onSettled: () => {
      invalidateAll(qc);
      void qc.invalidateQueries({ queryKey: ["scripts"] });
      void qc.invalidateQueries({ queryKey: ["history"] });
    },
  });

  return (
    <>
      <DialogHeader>
        <DialogTitle>Import from {preview.fileName}</DialogTitle>
        <DialogDescription>
          {preview.kind === "export"
            ? `RidgeSSH export${preview.createdAt ? ` made ${new Date(preview.createdAt).toLocaleString()}` : ""}${preview.appVersion ? ` (version ${preview.appVersion})` : ""}.`
            : "Plain ssh config: only concrete Host blocks are imported."}{" "}
          A backup is made first, and the import can be undone from History.
        </DialogDescription>
      </DialogHeader>

      {preview.warnings.length > 0 && (
        <ul className="grid gap-1 rounded-md border border-amber-500/40 bg-amber-500/5 p-2 text-xs">
          {preview.warnings.map((w) => (
            <li key={w} className="flex items-start gap-1.5">
              <TriangleAlertIcon className="mt-0.5 size-3.5 shrink-0 text-amber-500" />
              {w}
            </li>
          ))}
        </ul>
      )}

      <div className="flex flex-wrap items-end gap-3">
        <div className="grid min-w-56 gap-1.5">
          <Label htmlFor={groupId}>Put imported groups and hosts under</Label>
          <GroupSelect
            id={groupId}
            value={targetGroupId}
            onChange={setTargetGroupId}
            noneLabel="Top level"
          />
        </div>
        {existing.length > 0 && (
          <div className="flex items-center gap-1 text-sm">
            <span className="text-muted-foreground">{existing.length} already here:</span>
            {(["skip", "overwrite", "copy"] as const).map((s) => (
              <Button key={s} size="sm" variant="ghost" onClick={() => setExisting(s)}>
                {s === "skip" ? "Skip all" : s === "overwrite" ? "Overwrite all" : "Copy all"}
              </Button>
            ))}
          </div>
        )}
      </div>

      <div className="max-h-72 overflow-y-auto rounded-md border">
        <table className="w-full text-sm" aria-label="Hosts to import">
          <thead className="sticky top-0 bg-background text-xs text-muted-foreground">
            <tr className="border-b text-left">
              <th className="px-2 py-1.5 font-medium">Host</th>
              <th className="px-2 py-1.5 font-medium">Group</th>
              <th className="px-2 py-1.5 font-medium">Status</th>
              <th className="px-2 py-1.5 font-medium">Action</th>
            </tr>
          </thead>
          <tbody>
            {preview.hosts.length === 0 && (
              <tr>
                <td colSpan={4} className="p-4 text-center text-muted-foreground">
                  No hosts in this file.
                </td>
              </tr>
            )}
            {preview.hosts.map((h) => (
              <tr key={h.key} className="border-b last:border-0">
                <td className="px-2 py-1">
                  <div className="font-mono text-xs">{h.alias}</div>
                  <div className="truncate text-xs text-muted-foreground">
                    {[h.user, h.hostname].filter(Boolean).join("@")}
                    {h.port && h.port !== 22 ? `:${h.port}` : ""}
                  </div>
                  {h.note && hostChoice[h.key] === "overwrite" && (
                    <div className="text-xs text-amber-600 dark:text-amber-400">{h.note}</div>
                  )}
                </td>
                <td className="px-2 py-1 text-xs text-muted-foreground">
                  {h.groupPath.join(" / ") || "—"}
                </td>
                <td className="px-2 py-1">
                  <StatusBadge status={h.status} differences={h.differences} />
                </td>
                <td className="px-2 py-1">
                  <StrategySelect
                    status={h.status}
                    value={hostChoice[h.key]}
                    copyName={h.copyAlias}
                    label={`Action for ${h.alias}`}
                    onChange={(s) => setHostChoice((c) => ({ ...c, [h.key]: s }))}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {preview.scripts.length > 0 && (
        <div className="grid gap-1.5">
          <span className="text-sm font-medium">Scripts</span>
          {preview.scripts.map((s) => (
            <div key={s.key} className="flex items-center gap-2 text-sm">
              <span className="min-w-0 flex-1 truncate">{s.name}</span>
              <StatusBadge status={s.status} differences={s.differences} />
              <StrategySelect
                status={s.status}
                value={scriptChoice[s.key]}
                copyName={s.copyName}
                label={`Action for script ${s.name}`}
                onChange={(v) => setScriptChoice((c) => ({ ...c, [s.key]: v }))}
              />
            </div>
          ))}
        </div>
      )}

      {preview.keys.length > 0 && (
        <div className="grid gap-1 text-sm">
          <label className="flex items-center gap-2">
            <Checkbox
              checked={installKeys}
              disabled={newKeys === 0}
              onCheckedChange={(v) => setInstallKeys(v === true)}
            />
            Install {newKeys} key file{newKeys === 1 ? "" : "s"} into ~/.ssh
          </label>
          <ul className="grid gap-0.5 pl-6 text-xs text-muted-foreground">
            {preview.keys.map((k) => (
              <li key={k.identityFile}>
                <span className="font-mono">{k.identityFile}</span> —{" "}
                {k.status === "new"
                  ? "will be added"
                  : k.status === "same"
                    ? "already there"
                    : k.status === "exists"
                      ? "a different file is there; it is left unchanged"
                      : "outside ~/.ssh; not installed"}
              </li>
            ))}
          </ul>
        </div>
      )}

      <DialogFooter className="items-center">
        <span className="mr-auto text-xs text-muted-foreground">{plan.text}</span>
        <Button variant="outline" onClick={onBack}>
          Back
        </Button>
        <Button disabled={plan.count === 0 || apply.isPending} onClick={() => apply.mutate()}>
          <FileUpIcon /> {apply.isPending ? "Importing…" : "Import"}
        </Button>
      </DialogFooter>
    </>
  );
}

function reportResult(r: ImportResult) {
  const parts = [
    r.created && `${r.created} new`,
    r.overwritten && `${r.overwritten} overwritten`,
    r.copied && `${r.copied} copied`,
  ].filter(Boolean);
  const hosts = parts.length ? `Hosts: ${parts.join(", ")}` : "No hosts imported";
  const extra = [
    r.scripts && `${r.scripts} script${r.scripts === 1 ? "" : "s"}`,
    r.groupsCreated && `${r.groupsCreated} new group${r.groupsCreated === 1 ? "" : "s"}`,
    r.keysInstalled.length &&
      `${r.keysInstalled.length} key file${r.keysInstalled.length === 1 ? "" : "s"}`,
  ].filter(Boolean);
  const pending = r.pendingHostIds;
  toast.success(`${hosts}${extra.length ? `; ${extra.join(", ")}` : ""}`, {
    description: pending.length
      ? `${pending.length} host${pending.length === 1 ? " has" : "s have"} connection settings to write to ~/.ssh/config.`
      : undefined,
    action: pending.length
      ? { label: "Review", onClick: () => useUiStore.getState().openConfigWrite(pending) }
      : undefined,
    duration: pending.length ? 15000 : undefined,
  });
  for (const w of r.warnings) toast.warning(w);
}
