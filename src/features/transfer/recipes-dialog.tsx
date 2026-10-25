import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BookmarkIcon, PencilIcon, PlayIcon, Trash2Icon } from "lucide-react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { formatTs } from "@/features/hosts/format";
import { keys, useHostsData } from "@/features/hosts/queries";
import { useTerminalStore } from "@/features/terminal/store";
import { api, errorMessage, type FolderRecipe } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { tabTitle } from "./folder-plan";

/** Saved folder transfers: run one as saved, or open it to change settings first. */
export function RecipesDialog() {
  const open = useUiStore((s) => s.recipesOpen);
  const setOpen = useUiStore((s) => s.setRecipesOpen);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="flex max-h-[80vh] flex-col gap-3 sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Folder recipes</DialogTitle>
          <DialogDescription>
            Saved folder transfers. Run one as saved, or open it to change something for this run.
            Save new ones from a folder transfer with “Save as recipe”.
          </DialogDescription>
        </DialogHeader>
        {open && <List onClose={() => setOpen(false)} />}
      </DialogContent>
    </Dialog>
  );
}

function List({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient();
  const recipes = useQuery({ queryKey: keys.recipes, queryFn: api.recipesList });
  const hosts = useHostsData();
  const [deleting, setDeleting] = useState<FolderRecipe | null>(null);
  const live = useMemo(
    () => new Map(hosts.filter((h) => !h.deletedAt).map((h) => [h.id, h])),
    [hosts],
  );
  const alias = (id: string) => live.get(id)?.alias ?? "a deleted host";
  const remove = useMutation({
    mutationFn: api.recipeDelete,
    onSuccess: () => void qc.invalidateQueries({ queryKey: keys.recipes }),
    onError: (e) => toast.error(errorMessage(e)),
  });

  /** Hosts a recipe needs that no longer exist. */
  const missing = (r: FolderRecipe) =>
    [r.spec.source, r.spec.destination].some((e) => e.kind === "remote" && !live.has(e.hostId));

  const run = (r: FolderRecipe) => {
    const anchor =
      r.spec.source.kind === "remote"
        ? r.spec.source.hostId
        : (r.spec.destination as { hostId: string }).hostId;
    useTerminalStore
      .getState()
      .openJob(
        { id: anchor, alias: alias(anchor) },
        { kind: "folder", spec: r.spec, recipeId: r.id },
        tabTitle(r.spec, alias),
      );
    onClose();
  };

  const list = recipes.data ?? [];
  if (recipes.isSuccess && list.length === 0) {
    return (
      <div className="flex flex-col items-center gap-2 py-10 text-center text-sm text-muted-foreground">
        <BookmarkIcon className="size-8" />
        No recipes yet. Set up a folder transfer, then use “Save as recipe”.
      </div>
    );
  }

  return (
    <>
      <ScrollArea className="min-h-0 flex-1">
        <ul aria-label="Recipes" className="divide-y rounded-md border">
          {list.map((r) => (
            <li key={r.id} className="flex items-center gap-3 px-3 py-2">
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="truncate font-medium">{r.name}</span>
                  <LastRun recipe={r} />
                </div>
                <div className="truncate text-xs text-muted-foreground">
                  {tabTitle(r.spec, alias)}
                  {r.spec.source.path && ` · ${r.spec.source.path}`}
                </div>
              </div>
              <Button
                size="sm"
                variant="outline"
                disabled={missing(r)}
                title={missing(r) ? "A server it uses was deleted" : "Run as saved"}
                aria-label={`Run ${r.name}`}
                onClick={() => run(r)}
              >
                <PlayIcon /> Run
              </Button>
              <Button
                size="icon-sm"
                variant="ghost"
                disabled={missing(r)}
                aria-label={`Open ${r.name}`}
                title="Open to change settings before running"
                onClick={() => useUiStore.getState().openRecipe(r)}
              >
                <PencilIcon />
              </Button>
              <Button
                size="icon-sm"
                variant="ghost"
                aria-label={`Delete ${r.name}`}
                onClick={() => setDeleting(r)}
              >
                <Trash2Icon />
              </Button>
            </li>
          ))}
        </ul>
      </ScrollArea>
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(o) => !o && setDeleting(null)}
        title={`Delete the recipe "${deleting?.name}"?`}
        description="Only the saved settings are deleted; nothing on the servers changes."
        confirmLabel="Delete"
        destructive
        onConfirm={() => {
          if (deleting) remove.mutate(deleting.id);
          setDeleting(null);
        }}
      />
    </>
  );
}

function LastRun({ recipe }: { recipe: FolderRecipe }) {
  if (!recipe.lastRunAt) return null;
  const when = formatTs(recipe.lastRunAt);
  if (recipe.lastExitCode === 0)
    return (
      <Badge variant="secondary" title={`Last run ${when}`}>
        Done {when}
      </Badge>
    );
  if (recipe.lastExitCode === null)
    return (
      <Badge variant="outline" title={`Last run ${when}`}>
        Cancelled or running
      </Badge>
    );
  return (
    <Badge variant="destructive" title={`Last run ${when}`}>
      Failed {when}
    </Badge>
  );
}
