import { useMemo } from "react";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { buildGroupTree, descendantIds, flattenTree } from "@/features/hosts/filter";
import { useGroupsData } from "@/features/hosts/queries";

const NONE = "__none__";

interface GroupSelectProps {
  value: string | null;
  onChange: (groupId: string | null) => void;
  /** Hide this group and its descendants (used when picking a parent for a group). */
  exclude?: string;
  noneLabel?: string;
  id?: string;
}

export function GroupSelect({
  value,
  onChange,
  exclude,
  noneLabel = "No group",
  id,
}: GroupSelectProps) {
  const groups = useGroupsData();
  const options = useMemo(() => {
    const hidden = exclude ? descendantIds(groups, exclude) : new Set<string>();
    return flattenTree(buildGroupTree(groups)).filter((g) => !hidden.has(g.id));
  }, [groups, exclude]);

  return (
    <Select value={value ?? NONE} onValueChange={(v) => onChange(v === NONE ? null : v)}>
      <SelectTrigger id={id} className="w-full">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={NONE}>{noneLabel}</SelectItem>
        {options.map((g) => (
          <SelectItem key={g.id} value={g.id}>
            <span style={{ paddingLeft: g.depth * 12 }}>{g.name}</span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
