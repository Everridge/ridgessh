import { useId } from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { useScripts } from "@/features/scripts/queries";
import type { HookChoice } from "./folder-plan";

const NONE = "__none__";
const COMMAND = "__command__";

/** "Script before/after": nothing, a library script, or a command line. */
export function HookPicker({
  label,
  hint,
  value,
  onChange,
}: {
  label: string;
  hint: string;
  value: HookChoice;
  onChange: (c: HookChoice) => void;
}) {
  const id = useId();
  const commandId = useId();
  const scripts = useScripts().data ?? [];
  const selected =
    value.kind === "script" ? value.scriptId : value.kind === "command" ? COMMAND : NONE;
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      <Select
        value={selected}
        onValueChange={(v) =>
          onChange(
            v === NONE
              ? { kind: "none" }
              : v === COMMAND
                ? { kind: "command", command: value.kind === "command" ? value.command : "" }
                : { kind: "script", scriptId: v },
          )
        }
      >
        <SelectTrigger id={id} className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={NONE}>None</SelectItem>
          <SelectItem value={COMMAND}>A command…</SelectItem>
          {scripts.map((s) => (
            <SelectItem key={s.id} value={s.id}>
              Script: {s.name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {value.kind === "command" && (
        <Input
          id={commandId}
          aria-label={`${label}: command`}
          value={value.command}
          onChange={(e) => onChange({ kind: "command", command: e.target.value })}
          placeholder="sudo systemctl restart nginx"
          className="font-mono"
          spellCheck={false}
        />
      )}
      <p className="text-xs text-muted-foreground">{hint}</p>
    </div>
  );
}

export function ExcludesInput({
  value,
  onChange,
}: {
  value: string;
  onChange: (v: string) => void;
}) {
  const id = useId();
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>Leave out</Label>
      <Textarea
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={"node_modules\n.git\n*.log\nstorage/cache"}
        rows={3}
        className="font-mono text-xs"
        spellCheck={false}
      />
      <p className="text-xs text-muted-foreground">
        One per line. A name (node_modules, *.log) is left out wherever it appears; a path with a
        slash (storage/cache) is relative to the folder.
      </p>
    </div>
  );
}
