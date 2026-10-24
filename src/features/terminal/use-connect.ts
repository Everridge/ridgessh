import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import { invalidateAll } from "@/features/hosts/queries";
import { api, errorMessage, type Host } from "@/lib/ipc";
import { useTerminalStore } from "./store";

/** Opening a host in the embedded terminal or in an external console window. */
export function useConnect() {
  const qc = useQueryClient();
  return {
    connect: (host: Pick<Host, "id" | "alias">) => useTerminalStore.getState().open(host),
    connectExternal: (host: Pick<Host, "id" | "alias">) =>
      api
        .terminalOpenExternal(host.id)
        .then(() => invalidateAll(qc))
        .catch((e) => toast.error(`Could not open terminal: ${errorMessage(e)}`)),
  };
}
