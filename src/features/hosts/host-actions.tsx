import { useState } from "react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/confirm-dialog";
import type { Host } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";
import { useDeleteHosts, usePurgeHost, useRestoreHost } from "./queries";

/** Trash (with undo) / restore actions shared by the list, context menus and details panel. */
export function useHostActions() {
  const del = useDeleteHosts();
  const restore = useRestoreHost();
  // Read the store at call time so the callbacks never see stale state.
  const ui = useUiStore.getState;

  const trash = (hosts: Host[]) => {
    if (!hosts.length) return;
    const ids = hosts.map((h) => h.id);
    del.mutate(ids, {
      onSuccess: () => {
        ui().setChecked(ui().checkedIds.filter((id) => !ids.includes(id)));
        if (ids.includes(ui().selectedHostId ?? "")) ui().selectHost(null);
        toast(
          hosts.length === 1
            ? `"${hosts[0].alias}" moved to trash`
            : `${hosts.length} hosts moved to trash`,
          {
            action: { label: "Undo", onClick: () => ids.forEach((id) => restore.mutate(id)) },
          },
        );
      },
    });
  };

  return { trash, restore: (h: Host) => restore.mutate(h.id) };
}

/** Confirmation for permanent deletion; render once and call `ask(host)`. */
export function usePurgeConfirm() {
  const [host, setHost] = useState<Host | null>(null);
  const purge = usePurgeHost();
  const selectHost = useUiStore((s) => s.selectHost);

  const dialog = (
    <ConfirmDialog
      open={host !== null}
      onOpenChange={(o) => !o && setHost(null)}
      title={`Permanently delete "${host?.alias}"?`}
      description="Its notes, custom fields and tags are removed from RidgeSSH. The change history is kept. This does not touch ~/.ssh/config."
      confirmLabel="Delete permanently"
      destructive
      onConfirm={() => {
        if (host) purge.mutate(host.id, { onSuccess: () => selectHost(null) });
        setHost(null);
      }}
    />
  );

  return { ask: setHost, dialog };
}
