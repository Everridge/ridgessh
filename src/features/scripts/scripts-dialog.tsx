import { useId, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FileUpIcon, PlayIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
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
import { Textarea } from "@/components/ui/textarea";
import { api, errorMessage, type RunMode, type Script, type ScriptInput } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/ui";
import { MODE_LABELS, useDeleteScript, useSaveScript, useScripts } from "./queries";

const EMPTY: ScriptInput = {
  name: "",
  description: "",
  content: "#!/usr/bin/env bash\nset -euo pipefail\n\n",
  interpreter: "bash",
  defaultArgs: "",
  runMode: "stream",
};

export function ScriptsDialog() {
  const open = useUiStore((s) => s.scriptsOpen);
  const setOpen = useUiStore((s) => s.setScriptsOpen);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="flex h-[85vh] flex-col gap-3 sm:max-w-5xl">
        <DialogHeader>
          <DialogTitle>Scripts</DialogTitle>
          <DialogDescription>
            Shell scripts you can run on one or more hosts. Each run opens a terminal tab per host.
          </DialogDescription>
        </DialogHeader>
        {open && <Library />}
      </DialogContent>
    </Dialog>
  );
}

function Library() {
  const scripts = useScripts().data ?? [];
  // null = new script
  const [selected, setSelected] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const current = scripts.find((s) => s.id === selected) ?? null;
  const showEditor = editing || current !== null;

  return (
    <div className="grid min-h-0 flex-1 grid-cols-[15rem_1fr] gap-3">
      <div className="flex min-h-0 flex-col gap-2">
        <Button
          size="sm"
          variant="outline"
          onClick={() => {
            setSelected(null);
            setEditing(true);
          }}
        >
          <PlusIcon /> New script
        </Button>
        <ScrollArea className="min-h-0 flex-1 rounded-md border">
          {scripts.length === 0 ? (
            <p className="p-4 text-center text-sm text-muted-foreground">No scripts yet.</p>
          ) : (
            <ul aria-label="Scripts" className="divide-y">
              {scripts.map((s) => (
                <li key={s.id}>
                  <button
                    type="button"
                    onClick={() => {
                      setSelected(s.id);
                      setEditing(false);
                    }}
                    className={cn(
                      "grid w-full gap-0.5 px-3 py-2 text-left text-sm hover:bg-muted",
                      s.id === selected && "bg-muted",
                    )}
                  >
                    <span className="truncate font-medium">{s.name}</span>
                    {s.description && (
                      <span className="truncate text-xs text-muted-foreground">
                        {s.description}
                      </span>
                    )}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </ScrollArea>
      </div>
      {showEditor ? (
        <Editor
          key={current ? `${current.id}:${current.updatedAt}` : "new"}
          script={current}
          onSaved={(s) => {
            setSelected(s.id);
            setEditing(false);
          }}
          onDeleted={() => setSelected(null)}
        />
      ) : (
        <p className="flex items-center justify-center text-sm text-muted-foreground">
          Select a script, or create one.
        </p>
      )}
    </div>
  );
}

function Editor({
  script,
  onSaved,
  onDeleted,
}: {
  script: Script | null;
  onSaved: (s: Script) => void;
  onDeleted: () => void;
}) {
  const [v, setV] = useState<ScriptInput>(script ?? EMPTY);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const save = useSaveScript();
  const del = useDeleteScript();
  const id = useId();
  const set = <K extends keyof ScriptInput>(k: K, value: ScriptInput[K]) =>
    setV((x) => ({ ...x, [k]: value }));
  const dirty = !script || JSON.stringify(v) !== JSON.stringify(pick(script));

  const importFile = async () => {
    const path = await openDialog({
      title: "Import a script",
      filters: [{ name: "Shell scripts", extensions: ["sh", "bash", "txt"] }],
    });
    if (typeof path !== "string") return;
    try {
      const content = await api.scriptReadFile(path);
      setV((x) => ({ ...x, content, name: x.name || (path.split(/[\\/]/).pop() ?? "") }));
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const runOn = () => {
    const ui = useUiStore.getState();
    ui.setScriptsOpen(false);
    ui.openScriptRun(
      ui.checkedIds.length ? ui.checkedIds : ui.selectedHostId ? [ui.selectedHostId] : [],
      script?.id,
    );
  };

  return (
    <form
      className="flex min-h-0 flex-col gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        save.mutate({ id: script?.id ?? null, input: v }, { onSuccess: onSaved });
      }}
    >
      <div className="grid gap-3 sm:grid-cols-2">
        <div className="grid gap-1.5">
          <Label htmlFor={`${id}-name`}>Name</Label>
          <Input id={`${id}-name`} value={v.name} onChange={(e) => set("name", e.target.value)} />
        </div>
        <div className="grid gap-1.5">
          <Label htmlFor={`${id}-desc`}>Description</Label>
          <Input
            id={`${id}-desc`}
            value={v.description}
            onChange={(e) => set("description", e.target.value)}
          />
        </div>
        <div className="grid gap-1.5">
          <Label>Run mode</Label>
          <Select value={v.runMode} onValueChange={(m) => set("runMode", m as RunMode)}>
            <SelectTrigger aria-label="Run mode">
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
        <div className="grid grid-cols-[7rem_1fr] gap-2">
          <div className="grid gap-1.5">
            <Label htmlFor={`${id}-interp`}>Interpreter</Label>
            <Input
              id={`${id}-interp`}
              value={v.interpreter}
              onChange={(e) => set("interpreter", e.target.value)}
              className="font-mono"
            />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor={`${id}-args`}>Default arguments</Label>
            <Input
              id={`${id}-args`}
              value={v.defaultArgs}
              onChange={(e) => set("defaultArgs", e.target.value)}
              className="font-mono"
              placeholder={`e.g. --verbose "two words"`}
            />
          </div>
        </div>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-1.5">
        <div className="flex items-center">
          <Label htmlFor={`${id}-content`}>Script</Label>
          <Button
            type="button"
            size="xs"
            variant="ghost"
            className="ml-auto"
            onClick={() => void importFile()}
          >
            <FileUpIcon /> Import .sh file…
          </Button>
        </div>
        <Textarea
          id={`${id}-content`}
          value={v.content}
          onChange={(e) => set("content", e.target.value)}
          spellCheck={false}
          className="min-h-0 flex-1 resize-none font-mono text-xs"
        />
        <p className="text-xs text-muted-foreground">
          Windows line endings are converted before sending. Up to 20 KB.
        </p>
      </div>
      <div className="flex gap-2">
        {script && (
          <Button
            type="button"
            variant="ghost"
            className="text-destructive"
            onClick={() => setConfirmDelete(true)}
          >
            <Trash2Icon /> Delete
          </Button>
        )}
        <div className="ml-auto flex gap-2">
          {script && (
            <Button
              type="button"
              variant="outline"
              disabled={dirty}
              onClick={runOn}
              title={dirty ? "Save first" : undefined}
            >
              <PlayIcon /> Run…
            </Button>
          )}
          <Button type="submit" disabled={!dirty || save.isPending}>
            Save
          </Button>
        </div>
      </div>
      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        title={`Delete "${script?.name}"?`}
        description="The script is removed from the library. Its history entry keeps a copy of the content."
        confirmLabel="Delete"
        destructive
        onConfirm={() => {
          if (script) del.mutate(script.id, { onSuccess: onDeleted });
          setConfirmDelete(false);
        }}
      />
    </form>
  );
}

function pick(s: Script): ScriptInput {
  const { name, description, content, interpreter, defaultArgs, runMode } = s;
  return { name, description, content, interpreter, defaultArgs, runMode };
}
