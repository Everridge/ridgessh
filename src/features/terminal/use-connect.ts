import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import { invalidateAll } from "@/features/hosts/queries";
import { api, errorMessage, type Host } from "@/lib/ipc";
import { useTerminalStore } from "./store";

/** Opening a host in the embedded terminal or in an external console window. */
export function useConnect() {
  const qc = useQueryClient();
  return {
    /** `user`: connect as `user@alias` (a per-user `Match` block). */
    connect: (host: Pick<Host, "id" | "alias">, user?: string | null) =>
      useTerminalStore.getState().open(host, user),
    connectExternal: (host: Pick<Host, "id" | "alias">, user?: string | null) =>
      api
        .terminalOpenExternal(host.id, user ?? null)
        .then(() => invalidateAll(qc))
        .catch((e) => toast.error(`Could not open terminal: ${errorMessage(e)}`)),
  };
}
