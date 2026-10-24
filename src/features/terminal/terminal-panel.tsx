import { SquareTerminalIcon } from "lucide-react";

export function TerminalPanel() {
  return (
    <section className="flex h-full flex-col bg-muted/30">
      <div className="flex h-9 items-center border-b px-3">
        <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
          Terminal
        </h2>
      </div>
      <div className="flex flex-1 items-center justify-center gap-2 text-sm text-muted-foreground">
        <SquareTerminalIcon className="size-4" />
        No open sessions.
      </div>
    </section>
  );
}
