import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invalidateAll, keys } from "@/features/hosts/queries";
import { api, type ConfigChange } from "@/lib/ipc";

export const useConfigCandidates = () =>
  useQuery({ queryKey: keys.configCandidates, queryFn: api.configCandidates });

/** Read-only diff for the selected changes. */
export const useConfigPreview = (changes: ConfigChange[]) =>
  useQuery({
    queryKey: [...keys.configPreview, changes],
    queryFn: () => api.configPreview(changes),
    enabled: changes.length > 0,
    placeholderData: keepPreviousData,
  });

export function useApplyConfig() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: {
      changes: ConfigChange[];
      baseHash: string;
      resultHash: string;
      confirmation: string;
    }) => api.configApply(v.changes, v.baseHash, v.resultHash, v.confirmation),
    onSuccess: (r) => qc.setQueryData(keys.lastSync, r.report),
    onSettled: () => invalidateAll(qc),
  });
}
