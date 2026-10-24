import type { Host } from "@/lib/ipc";

/** `user@hostname:port`, omitting parts that are not set (port 22 is implied). */
export function connectionLabel(h: Host): string {
  const host = h.hostname ?? h.alias;
  return `${h.user ? `${h.user}@` : ""}${host}${h.port && h.port !== 22 ? `:${h.port}` : ""}`;
}

/** Short local date-time for timestamps from the backend. */
export function formatTs(ts: string | null): string {
  if (!ts) return "—";
  const d = new Date(ts);
  return Number.isNaN(d.getTime()) ? ts : d.toLocaleString();
}
