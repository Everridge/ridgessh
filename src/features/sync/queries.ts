import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { invalidateAll, keys, useDataMutation } from "@/features/hosts/queries";
import { api, errorMessage } from "@/lib/ipc";

export const useLastSync = () => useQuery({ queryKey: keys.lastSync, queryFn: api.syncLast });

export const useRenameSuggestions = () =>
  useQuery({ queryKey: keys.renames, queryFn: api.syncRenameSuggestions });

/** Scans ~/.ssh/config and reconciles the DB. Callers decide how to report the result. */
export function useRunSync() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: api.syncRun,
    onSuccess: (report) => qc.setQueryData(keys.lastSync, report),
    onError: (e) => toast.error(`Could not read SSH config: ${errorMessage(e)}`),
    onSettled: () => invalidateAll(qc),
  });
}

export const useAcknowledge = () => useDataMutation((ids: string[]) => api.syncAcknowledge(ids));

export const useKeepDbOnly = () =>
  useDataMutation(
    (ids: string[]) => api.syncKeepDbOnly(ids),
    (_, ids) => (ids.length === 1 ? "Kept as RidgeSSH-only host" : `Kept ${ids.length} hosts`),
  );

export const useResolveConflict = () =>
  useDataMutation(
    ({ id, keep }: { id: string; keep: "config" | "database" }) =>
      api.syncResolveConflict(id, keep),
    (h, { keep }) =>
      keep === "config"
        ? `"${h.alias}" now matches the SSH config`
        : `"${h.alias}" keeps your version (pending write to config)`,
  );

export const useAcceptRename = () =>
  useDataMutation(
    ({ fromId, toId }: { fromId: string; toId: string }) => api.syncAcceptRename(fromId, toId),
    (h) => `Notes and history carried over to "${h.alias}"`,
  );
