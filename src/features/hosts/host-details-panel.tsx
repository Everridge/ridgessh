import { MousePointerClickIcon } from "lucide-react";

export function HostDetailsPanel() {
  return (
    <section className="flex h-full flex-col">
      <div className="flex h-10 items-center border-b px-3">
        <h2 className="text-sm font-medium">Details</h2>
      </div>
      <div className="flex flex-1 flex-col items-center justify-center gap-2 p-6 text-center text-sm text-muted-foreground">
        <MousePointerClickIcon className="size-8" />
        Select a host to see its details.
      </div>
    </section>
  );
}
