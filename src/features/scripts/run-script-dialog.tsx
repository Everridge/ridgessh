import { useId, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FileUpIcon, PlayIcon, SearchIcon } from "lucide-react";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { connectionLabel } from "@/features/hosts/format";
import { useHostsData } from "@/features/hosts/queries";
import { useTerminalStore } from "@/features/terminal/store";
import { api, errorMessage, type RunMode, type ScriptRun } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { MODE_LABELS, useScripts } from "./queries";

const LOCAL = "__local__";

export function RunScriptDialog() {
  const target = useUiStore((s) => s.scriptRun);
  const close = useUiStore((s) => s.closeScriptRun);
  return (
    <Dialog open={target !== null} onOpenChange={(o) => !o && close()}>
      <DialogContent className="flex max-h-[85vh] flex-col gap-3 sm:max-w-2xl">
        {target && (
          <Body initialHosts={target.hostIds} initialScript={target.scriptId} onDone={close} />
        )}
      </DialogContent>
    </Dialog>
  );
}

function Body({
  initialHosts,
  initialScript,
  onDone,
}: {
  initialHosts: string[];
  initialScript?: string;
  onDone: () => void;
}) {
  const scripts = useScripts().data ?? [];
  const hosts = useHostsData().filter((h) => !h.deletedAt);
  const [scriptId, setScriptId] = useState<string | undefined>(initialScript);
  const [local, setLocal] = useState<{ name: string; content: string } | null>(null);
  const [args, setArgs] = useState<string | null>(null);
  const [mode, setMode] = useState<RunMode | null>(null);
  const [targets, setTargets] = useState<string[]>(initialHosts);
  const [filter, setFilter] = useState("");
  const argsId = useId();

  const script = scripts.find((s) => s.id === scriptId);
  const effectiveMode = mode ?? script?.runMode ?? "stream";
  const effectiveArgs = args ?? script?.defaultArgs ?? "";
  const chosen = hosts.filter((h) => targets.includes(h.id));
  const q = filter.trim().toLowerCase();
  const shown = q
    ? hosts.filter((h) => `${h.alias} ${connectionLabel(h)}`.toLowerCase().includes(q))
    : hosts;
  const name = local?.name ?? script?.name;
  const ready = !!name && chosen.length > 0;

  const pickLocal = async () => {
    const path = await openDialog({
      title: "Run a local script",
      filters: [{ name: "Shell scripts", extensions: ["sh", "bash", "txt"] }],
    });
    if (typeof path !== "string") return;
    try {
      const content = await api.scriptReadFile(path);
      setLocal({ name: path.split(/[\\/]/).pop() ?? path, content });
      setScriptId(undefined);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const run = () => {
    const store = useTerminalStore.getState();
    for (const h of chosen) {
      const spec: ScriptRun = local
        ? {
            hostId: h.id,
            name: local.name,
            content: local.content,
            mode: effectiveMode,
            args: effectiveArgs,
          }
        : { hostId: h.id, scriptId, mode: effectiveMode, args: effectiveArgs };
      store.openJob(h, { kind: "script", run: spec }, `▶ ${name} · ${h.alias}`);
    }
    onDone();
  };

  const toggle = (id: string, on: boolean) =>
    setTargets((t) => (on ? [...t, id] : t.filter((x) => x !== id)));

  return (
    <>
      <DialogHeader>
        <DialogTitle>Run script</DialogTitle>
        <DialogDescription>
          Each host gets its own terminal tab, so you can answer prompts and see the output and exit
          code.
        </DialogDescription>
      </DialogHeader>

      <div className="grid gap-3 sm:grid-cols-2">
        <div className="grid gap-1.5">
          <Label>Script</Label>
          <div className="flex gap-2">
            <Select
              value={local ? LOCAL : (scriptId ?? "")}
              onValueChange={(v) => {
                if (v === LOCAL) return;
                setScriptId(v);
                setLocal(null);
                setArgs(null);
                setMode(null);
              }}
            >
              <SelectTrigger aria-label="Script" className="min-w-0 flex-1">
                <SelectValue placeholder="Choose a script" />
              </SelectTrigger>
              <SelectContent>
                {scripts.map((s) => (
                  <SelectItem key={s.id} value={s.id}>
                    {s.name}
                  </SelectItem>
                ))}
                {local && <SelectItem value={LOCAL}>{local.name} (local file)</SelectItem>}
              </SelectContent>
            </Select>
            <Button
              variant="outline"
              size="icon"
              aria-label="Run a local .sh file"
              title="Run a local .sh file"
              onClick={() => void pickLocal()}
            >
              <FileUpIcon />
            </Button>
          </div>
        </div>
        <div className="grid gap-1.5">
          <Label>Mode</Label>
          <Select value={effectiveMode} onValueChange={(m) => setMode(m as RunMode)}>
            <SelectTrigger aria-label="Mode">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(Object.keys(MODE_LABELS) as RunMode[]).map((m) => (
                <SelectItem key={m} value={m}>
                  {MODE_LABELS[m]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="grid gap-1.5 sm:col-span-2">
          <Label htmlFor={argsId}>Arguments</Label>
          <Input
            id={argsId}
            value={effectiveArgs}
            onChange={(e) => setArgs(e.target.value)}
            className="font-mono"
            spellCheck={false}
            placeholder={`Quoted like a shell command line, e.g. --env "staging eu"`}
          />
        </div>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <Label>Hosts</Label>
          <span className="text-xs text-muted-foreground">{chosen.length} selected</span>
          <div className="relative ml-auto w-48">
            <SearchIcon className="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              placeholder="Filter"
              aria-label="Filter hosts"
              className="h-7 pl-7 text-xs"
            />
          </div>
        </div>
        <ScrollArea className="h-56 rounded-md border">
          <ul aria-label="Target hosts" className="divide-y">
            {shown.map((h) => (
              <li key={h.id}>
                <label className="flex items-center gap-2 px-3 py-1.5 text-sm">
                  <Checkbox
                    checked={targets.includes(h.id)}
                    onCheckedChange={(v) => toggle(h.id, v === true)}
                    aria-label={h.alias}
                  />
                  <span className="font-medium">{h.alias}</span>
                  <span className="truncate text-xs text-muted-foreground">
                    {connectionLabel(h)}
                  </span>
                </label>
              </li>
            ))}
          </ul>
        </ScrollArea>
      </div>

      <DialogFooter className="items-center gap-3 sm:justify-between">
        <p className="min-w-0 truncate text-xs text-muted-foreground">
          {chosen.length > 0 && `Runs on: ${chosen.map((h) => h.alias).join(", ")}`}
        </p>
        <div className="flex shrink-0 gap-2">
          <Button variant="outline" onClick={onDone}>
            Cancel
          </Button>
          <Button disabled={!ready} onClick={run}>
            <PlayIcon />
            {chosen.length > 1 ? `Run on ${chosen.length} hosts` : "Run"}
          </Button>
        </div>
      </DialogFooter>
    </>
  );
}
