import { useInfiniteQuery } from "@tanstack/react-query";
import { useDataMutation } from "@/features/hosts/queries";
import { api, type AuditEntry, type HistoryFilter } from "@/lib/ipc";

const PAGE = 100;

/** Global history, newest first, loaded page by page. */
export const useHistory = (filter: HistoryFilter) =>
  useInfiniteQuery({
    queryKey: ["history", filter],
    queryFn: ({ pageParam }) => api.historyList({ ...filter, beforeId: pageParam, limit: PAGE }),
    initialPageParam: null as number | null,
    getNextPageParam: (last) => (last.length === PAGE ? last[last.length - 1].id : undefined),
  });

export const useRevert = () =>
  useDataMutation(
    ({ id, target }: { id: number; target: "before" | "after" }) => api.historyRevert(id, target),
    (_, { target }) => (target === "before" ? "Change undone" : "Version restored"),
  );

export const useRevertBatch = () =>
  useDataMutation(
    (batchId: string) => api.historyRevertBatch(batchId),
    (n) => `Undid ${n} change${n === 1 ? "" : "s"}`,
  );

/** Kinds of entities whose changes can be undone (mirrors `revert::supported`). */
export const canRevert = (e: AuditEntry) =>
  ["host", "group", "settings", "script"].includes(e.entityType);

export const SOURCE_LABELS: Record<AuditEntry["source"], string> = {
  user: "you",
  sync: "config scan",
  import: "import",
  restore: "undo / restore",
  system: "automatic",
};
