const UNITS = ["B", "KB", "MB", "GB"];

/** Human-readable byte size: 0 B, 512 B, 1.5 KB, 12 MB. */
export function formatBytes(n: number | null | undefined): string {
  if (n === null || n === undefined || !Number.isFinite(n) || n < 0) return "—";
  let value = n;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = unit === 0 || value >= 10 ? 0 : 1;
  return `${value.toFixed(digits)} ${UNITS[unit]}`;
}

/** Duration in whole ms → "<1 ms", "38 ms", "1.24 s". */
export function formatDuration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms)) return "—";
  if (ms < 1) return "<1 ms";
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

/** Epoch ms → local HH:MM:SS.mmm */
export function formatClock(epochMs: number): string {
  const d = new Date(epochMs);
  const pad = (v: number, w = 2) => String(v).padStart(w, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
}

/** Epoch ms → full local date and time. */
export function formatDateTime(epochMs: number): string {
  return new Date(epochMs).toLocaleString();
}

export type StatusTone = "info" | "ok" | "redirect" | "client" | "server" | "none";

/** Colour family for an HTTP status code. */
export function statusTone(status: number | null | undefined): StatusTone {
  if (status === null || status === undefined) return "none";
  if (status < 200) return "info";
  if (status < 300) return "ok";
  if (status < 400) return "redirect";
  if (status < 500) return "client";
  return "server";
}
