import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { api, errorMessage, type RunMode, type ScriptInput } from "@/lib/ipc";

const KEY = ["scripts"] as const;

export const useScripts = () => useQuery({ queryKey: KEY, queryFn: api.scriptsList });

export function useSaveScript() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id: string | null; input: ScriptInput }) =>
      id ? api.scriptUpdate(id, input) : api.scriptCreate(input),
    onSuccess: (s) => toast.success(`Script "${s.name}" saved`),
    onError: (e) => toast.error(errorMessage(e)),
    onSettled: () => void qc.invalidateQueries({ queryKey: KEY }),
  });
}

export function useDeleteScript() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api.scriptDelete(id),
    onSuccess: () => toast("Script deleted"),
    onError: (e) => toast.error(errorMessage(e)),
    onSettled: () => void qc.invalidateQueries({ queryKey: KEY }),
  });
}

export const MODE_LABELS: Record<RunMode, string> = {
  stream: "Stream (nothing left on the server)",
  copy: "Copy & run (can prompt, e.g. sudo)",
};
