import { useId } from "react";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

const DEFAULT = "__default__";

/**
 * "Connect as": the host's own user or one of its per-user `Match` blocks. Renders nothing
 * when there is no choice to make.
 */
export function UserSelect({
  users,
  value,
  onChange,
  defaultLabel,
}: {
  users: string[];
  value: string | null;
  onChange: (user: string | null) => void;
  /** What the default entry connects as, e.g. "root (from Host block)". */
  defaultLabel: string;
}) {
  const id = useId();
  if (users.length === 0) return null;
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>Connect as</Label>
      <Select value={value ?? DEFAULT} onValueChange={(v) => onChange(v === DEFAULT ? null : v)}>
        <SelectTrigger id={id} className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={DEFAULT}>{defaultLabel}</SelectItem>
          {users.map((u) => (
            <SelectItem key={u} value={u}>
              {u}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}
