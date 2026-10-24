import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useCreateGroup, useUpdateGroup } from "@/features/hosts/queries";
import type { Group } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { GroupSelect } from "./group-select";

const GROUP_COLORS = [
  "#64748b",
  "#ef4444",
  "#f59e0b",
  "#22c55e",
  "#06b6d4",
  "#3b82f6",
  "#8b5cf6",
  "#ec4899",
];

export type GroupDialogTarget =
  { mode: "create"; parentId: string | null } | { mode: "edit"; group: Group };

interface GroupDialogProps {
  target: GroupDialogTarget | null;
  onClose: () => void;
}

export function GroupDialog({ target, onClose }: GroupDialogProps) {
  return (
    <Dialog open={target !== null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        {/* Keyed so the form state resets for every new target. */}
        {target && (
          <GroupForm
            key={target.mode === "edit" ? target.group.id : `new-${target.parentId}`}
            target={target}
            onClose={onClose}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

function GroupForm({ target, onClose }: { target: GroupDialogTarget; onClose: () => void }) {
  const editing = target.mode === "edit" ? target.group : null;
  const [name, setName] = useState(editing?.name ?? "");
  const [parentId, setParentId] = useState(
    editing ? editing.parentId : target.mode === "create" ? target.parentId : null,
  );
  const [color, setColor] = useState<string | null>(editing?.color ?? null);
  const create = useCreateGroup();
  const update = useUpdateGroup();
  const pending = create.isPending || update.isPending;

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const input = { name, parentId, color, icon: editing?.icon ?? null };
    const done = { onSuccess: onClose };
    if (editing) update.mutate({ id: editing.id, input }, done);
    else create.mutate(input, done);
  };

  return (
    <form onSubmit={submit} className="grid gap-4">
      <DialogHeader>
        <DialogTitle>{editing ? "Edit group" : "New group"}</DialogTitle>
      </DialogHeader>
      <div className="grid gap-2">
        <Label htmlFor="group-name">Name</Label>
        <Input id="group-name" autoFocus value={name} onChange={(e) => setName(e.target.value)} />
      </div>
      <div className="grid gap-2">
        <Label htmlFor="group-parent">Parent</Label>
        <GroupSelect
          id="group-parent"
          value={parentId}
          onChange={setParentId}
          exclude={editing?.id}
          noneLabel="Top level"
        />
      </div>
      <div className="grid gap-2">
        <Label>Color</Label>
        <div className="flex flex-wrap gap-2">
          <ColorSwatch selected={color === null} onClick={() => setColor(null)} label="No color" />
          {GROUP_COLORS.map((c) => (
            <ColorSwatch
              key={c}
              color={c}
              selected={color === c}
              onClick={() => setColor(c)}
              label={c}
            />
          ))}
        </div>
      </div>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" disabled={pending || !name.trim()}>
          {editing ? "Save" : "Create"}
        </Button>
      </DialogFooter>
    </form>
  );
}

function ColorSwatch({
  color,
  selected,
  onClick,
  label,
}: {
  color?: string;
  selected: boolean;
  onClick: () => void;
  label: string;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={selected}
      onClick={onClick}
      className={cn(
        "size-6 rounded-full border-2 ring-offset-2 ring-offset-background",
        selected ? "ring-2 ring-ring" : "border-transparent",
        !color && "border-dashed border-muted-foreground/50",
      )}
      style={color ? { backgroundColor: color } : undefined}
    />
  );
}
