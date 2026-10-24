import { useHostHistory } from "@/features/hosts/queries";
import { HistoryEntry, HistoryEntryIcon } from "./history-entry";

export function HostHistory({ hostId }: { hostId: string }) {
  const history = useHostHistory(hostId);

  if (history.isLoading) return null;
  if (!history.data?.length) {
    return <p className="p-4 text-sm text-muted-foreground">No history yet.</p>;
  }

  return (
    <ol className="relative grid gap-4 p-4 before:absolute before:top-6 before:bottom-6 before:left-[27px] before:w-px before:bg-border">
      {history.data.map((e) => (
        <li key={e.id} className="relative flex gap-3">
          <HistoryEntryIcon entry={e} />
          <HistoryEntry entry={e} />
        </li>
      ))}
    </ol>
  );
}
