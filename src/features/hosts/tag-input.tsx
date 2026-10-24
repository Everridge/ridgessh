import { useState } from "react";
import { XIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { useTagsData } from "./queries";

interface TagInputProps {
  value: string[];
  onChange: (tags: string[]) => void;
  id?: string;
}

/** Comma/Enter separated tags with suggestions from existing tags. */
export function TagInput({ value, onChange, id }: TagInputProps) {
  const [draft, setDraft] = useState("");
  const known = useTagsData();
  const has = (t: string) => value.some((v) => v.toLowerCase() === t.toLowerCase());

  const add = (raw: string) => {
    const parts = raw
      .split(",")
      .map((s) => s.trim())
      .filter((s) => s && !has(s));
    if (parts.length) onChange([...value, ...parts]);
    setDraft("");
  };

  const suggestions = draft
    ? known
        .filter((t) => t.name.toLowerCase().includes(draft.toLowerCase()) && !has(t.name))
        .slice(0, 6)
    : [];

  return (
    <div className="grid gap-1.5">
      <div className="flex min-h-8 flex-wrap items-center gap-1 rounded-md border border-input bg-transparent px-2 py-1 focus-within:ring-2 focus-within:ring-ring/50 dark:bg-input/30">
        {value.map((t) => (
          <Badge key={t} variant="secondary" className="gap-1">
            {t}
            <button
              type="button"
              aria-label={`Remove ${t}`}
              onClick={() => onChange(value.filter((v) => v !== t))}
            >
              <XIcon className="size-3" />
            </button>
          </Badge>
        ))}
        <input
          id={id}
          value={draft}
          placeholder={value.length ? "" : "Add tags…"}
          className="min-w-24 flex-1 bg-transparent text-sm outline-none"
          onChange={(e) => {
            if (e.target.value.endsWith(",")) add(e.target.value);
            else setDraft(e.target.value);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              add(draft);
            } else if (e.key === "Backspace" && !draft && value.length) {
              onChange(value.slice(0, -1));
            }
          }}
          onBlur={() => draft && add(draft)}
        />
      </div>
      {suggestions.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {suggestions.map((t) => (
            <button
              key={t.id}
              type="button"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => add(t.name)}
            >
              <Badge variant="outline">+ {t.name}</Badge>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
