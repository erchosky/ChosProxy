import type { PerformanceSnapshot } from "../bindings/PerformanceSnapshot";

/** Readings kept in the Performance view (3 minutes at one every 2 s). */
export const PERF_HISTORY = 90;

/** Requests stored per second between two readings (0 if the counter went back). */
export function storedRate(prev: PerformanceSnapshot, cur: PerformanceSnapshot): number {
  const seconds = (cur.at - prev.at) / 1000;
  const added = cur.stored - prev.stored;
  if (seconds <= 0 || added < 0) return 0;
  return added / seconds;
}

/** Adds a reading, keeping the newest `max`. */
export function pushReading<T>(list: T[], item: T, max = PERF_HISTORY): T[] {
  const next = [...list, item];
  return next.length > max ? next.slice(next.length - max) : next;
}

/** SVG polyline points for `values` in a `width` × `height` box (0 at the bottom). */
export function sparkline(values: number[], width: number, height: number): string {
  if (values.length === 0) return "";
  const max = Math.max(...values, 1e-9);
  const step = values.length > 1 ? width / (values.length - 1) : 0;
  return values
    .map((v, i) => {
      const x = values.length > 1 ? i * step : width;
      const y = height - (Math.max(v, 0) / max) * height;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}
