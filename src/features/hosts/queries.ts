import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  api,
  errorMessage,
  type GroupInput,
  type Host,
  type HostInput,
  type UserVariant,
} from "@/lib/ipc";
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
  settings: ["settings"] as const,
  backupStatus: ["backup-status"] as const,
  backups: ["backups"] as const,
  variants: ["config-variants"] as const,
  recipes: ["folder-recipes"] as const,
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
    keys.variants,
    keys.recipes,
    // Edits can trigger automatic backups.
    keys.backupStatus,
    keys.backups,
    ["host-history"],
    ["history"],
    keys.settings,
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

const NO_VARIANTS: UserVariant[] = [];
/** Per-user `Match` blocks of the live config (read-only). */
export const useVariants = () =>
  useQuery({ queryKey: keys.variants, queryFn: api.configVariants }).data ?? NO_VARIANTS;

/** The config alias a host's `Match originalhost` blocks name. */
export const configAlias = (host: Pick<Host, "alias" | "configSnapshot">) =>
  host.configSnapshot?.alias ?? host.alias;

/** The user variants of `host`, by user name. */
export function variantsFor(host: Pick<Host, "alias" | "configSnapshot">, all: UserVariant[]) {
  const alias = configAlias(host);
  return all.filter((v) => v.alias === alias).sort((a, b) => a.user.localeCompare(b.user));
}
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

export const useEmptyTrash = () =>
  useDataMutation(api.hostsEmptyTrash, ({ purged, keptInConfig }) =>
    keptInConfig > 0
      ? `Deleted ${purged} host${purged === 1 ? "" : "s"}; ${keptInConfig} still in ~/.ssh/config stayed in trash`
      : `Deleted ${purged} host${purged === 1 ? "" : "s"} permanently`,
  );
