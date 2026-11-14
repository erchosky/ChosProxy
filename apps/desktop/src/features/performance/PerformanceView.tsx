import { useEffect, useState } from "react";
import type { PerformanceSnapshot } from "../../bindings/PerformanceSnapshot";
import { ErrorState, LoadingState } from "../../components/States";
import { formatBytes } from "../../lib/format";
import { pushReading, sparkline, storedRate } from "../../lib/perf";
import { api, toIpcError } from "../../services/ipc";

const EVERY_MS = 2000;

function Spark({ values, label }: { values: number[]; label: string }) {
  return (
    <svg className="perf-spark" viewBox="0 0 120 28" preserveAspectRatio="none" aria-label={label}>
      <polyline points={sparkline(values, 120, 26)} fill="none" />
    </svg>
  );
}

function Tile({
  label,
  value,
  hint,
  series,
}: {
  label: string;
  value: string;
  hint?: string;
  series?: number[];
}) {
  return (
    <div className="metric perf-tile" title={hint}>
      <div className="metric-value">{value}</div>
      <div className="metric-label">{label}</div>
      {series && series.length > 1 && <Spark values={series} label={`${label} over time`} />}
    </div>
  );
}

/** Tools → Performance (M31): live counters of this process, local only. */
export function PerformanceView() {
  const [readings, setReadings] = useState<PerformanceSnapshot[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const read = () =>
      api.performance().then(
        (s) => {
          if (!alive) return;
          setReadings((r) => pushReading(r, s));
          setError(null);
        },
        (e: unknown) => alive && setError(toIpcError(e).message),
      );
    void read();
    const timer = setInterval(() => void read(), EVERY_MS);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);

  if (error && readings.length === 0)
    return <ErrorState title="Could not read the counters" message={error} />;
  const cur = readings.at(-1);
  if (!cur) return <LoadingState />;
  const rates = readings.slice(1).map((r, i) => storedRate(readings[i] ?? r, r));
  const memory = readings.map((r) => r.process?.memoryBytes ?? 0);
  const cpu = readings.map((r) => r.process?.cpuPercent ?? 0);
  return (
    <div className="page">
      <div className="page-header">
        <div>
          <h1>Performance</h1>
          <p className="page-sub">
            This ChosProxy process, read every 2 s while this page is open. Nothing leaves the
            machine.
          </p>
        </div>
      </div>
      <section className="card">
        <div className="metrics perf-metrics">
          <Tile
            label="Memory"
            value={cur.process ? formatBytes(cur.process.memoryBytes) : "—"}
            series={memory}
          />
          <Tile
            label="CPU"
            value={cur.process ? `${cur.process.cpuPercent.toFixed(0)} %` : "—"}
            hint="Percent of one core; above 100 % uses several cores."
            series={cpu}
          />
          <Tile
            label="Stored per second"
            value={(rates.at(-1) ?? 0).toFixed(1)}
            hint="Requests and responses written to the project, from every tool."
            series={rates}
          />
          <Tile
            label="Project database"
            value={formatBytes(cur.dbBytes + cur.walBytes)}
            hint={`File ${formatBytes(cur.dbBytes)}, write-ahead log ${formatBytes(cur.walBytes)}`}
          />
          <Tile label="Proxy connections open" value={String(cur.proxy.activeConnections)} />
          <Tile label="Tasks running" value={String(cur.tasksRunning)} />
          <Tile
            label="Pool requests in flight / waiting"
            value={`${cur.poolInFlight} / ${cur.poolWaiting}`}
          />
          <Tile
            label="Captures dropped"
            value={String(cur.dropped)}
            hint="Stored traffic lost because the disk could not keep up."
          />
          <Tile label="Stored since the project opened" value={String(cur.stored)} />
        </div>
      </section>
      {error && <p className="callout callout-warn">Last reading failed: {error}</p>}
    </div>
  );
}
