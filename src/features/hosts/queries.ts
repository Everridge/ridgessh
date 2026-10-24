import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { api, errorMessage, type GroupInput, type HostInput } from "@/lib/ipc";
import { useUiStore } from "@/stores/ui";

export const keys = {
  hosts: ["hosts"] as const,
  groups: ["groups"] as const,
  tags: ["tags"] as const,
  hostHistory: (id: string) => ["host-history", id] as const,
  lastSync: ["sync-last"] as const,
  renames: ["sync-renames"] as const,
  configCandidates: ["config-candidates"] as const,
  configPreview: ["config-preview"] as const,
};

/** Refreshes everything derived from hosts after any change. */
export function invalidateAll(qc: QueryClient) {
  for (const queryKey of [
    keys.hosts,
    keys.groups,
    keys.tags,
    keys.renames,
    keys.configCandidates,
    keys.configPreview,
    ["host-history"],
  ]) {
    void qc.invalidateQueries({ queryKey });
  }
}

// Stable fallback so `data ?? EMPTY` does not change identity on every render.
const EMPTY: never[] = [];

export const useHosts = () => useQuery({ queryKey: keys.hosts, queryFn: api.hostsList });
export const useGroups = () => useQuery({ queryKey: keys.groups, queryFn: api.groupsList });
export const useTags = () => useQuery({ queryKey: keys.tags, queryFn: api.tagsList });

export const useHostsData = () => useHosts().data ?? EMPTY;
export const useGroupsData = () => useGroups().data ?? EMPTY;
export const useTagsData = () => useTags().data ?? EMPTY;
export const useHostHistory = (id: string) =>
  useQuery({ queryKey: keys.hostHistory(id), queryFn: () => api.hostHistory(id) });

type SuccessToast = string | { message: string; action: { label: string; onClick: () => void } };

/** Mutation that refreshes all host-related data and toasts errors. */
export function useDataMutation<V, R>(
  fn: (v: V) => Promise<R>,
  success?: (r: R, v: V) => SuccessToast,
) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: fn,
    onSuccess: (r, v) => {
      const msg = success?.(r, v);
      if (typeof msg === "string") toast.success(msg);
      else if (msg) toast.success(msg.message, { action: msg.action });
    },
    onError: (e) => toast.error(errorMessage(e)),
    onSettled: () => invalidateAll(qc),
  });
}

export const useCreateHost = () =>
  useDataMutation(
    (input: HostInput) => api.hostCreate(input),
    (h) => `Host "${h.alias}" created`,
  );

export const useUpdateHost = () =>
  useDataMutation(
    ({ id, input }: { id: string; input: HostInput }) => api.hostUpdate(id, input),
    (h) =>
      h.configDirty && h.syncState !== "conflict"
        ? {
            message: `Host "${h.alias}" saved. The connection changes are not in ~/.ssh/config yet.`,
            action: {
              label: "Write to config",
              onClick: () => useUiStore.getState().openConfigWrite([h.id]),
            },
          }
        : `Host "${h.alias}" saved`,
  );

export const useSetFavorite = () =>
  useDataMutation(({ id, favorite }: { id: string; favorite: boolean }) =>
    api.hostSetFavorite(id, favorite),
  );

export const useMoveHosts = () =>
  useDataMutation(({ ids, groupId }: { ids: string[]; groupId: string | null }) =>
    api.hostsMove(ids, groupId),
  );

// No success toast: callers show one with an Undo action (see host-actions.tsx).
export const useDeleteHosts = () => useDataMutation((ids: string[]) => api.hostsDelete(ids));

export const useRestoreHost = () =>
  useDataMutation(
    (id: string) => api.hostRestore(id),
    (h) => `Host "${h.alias}" restored`,
  );

export const usePurgeHost = () =>
  useDataMutation(
    (id: string) => api.hostPurge(id),
    () => "Host permanently deleted",
  );

export const useCreateGroup = () =>
  useDataMutation(
    (input: GroupInput) => api.groupCreate(input),
    (g) => `Group "${g.name}" created`,
  );

export const useUpdateGroup = () =>
  useDataMutation(({ id, input }: { id: string; input: GroupInput }) => api.groupUpdate(id, input));

export const useDeleteGroup = () =>
  useDataMutation(
    (id: string) => api.groupDelete(id),
    () => "Group deleted",
  );
