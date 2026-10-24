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

/** Human duration between two timestamps, e.g. "1h 4m", "35s". */
export function duration(start: string, end: string | null): string {
  if (!end) return "running";
  const s = Math.max(0, Math.round((Date.parse(end) - Date.parse(start)) / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h) return `${h}h ${m}m`;
  if (m) return `${m}m ${s % 60}s`;
  return `${s}s`;
}
