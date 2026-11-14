import { useEffect, useState } from "react";
import type { CrawlConfig } from "../../bindings/CrawlConfig";
import type { DiscoveryHit } from "../../bindings/DiscoveryHit";
import type { TaskRow } from "../../bindings/TaskRow";
import type { TaskStatus } from "../../bindings/TaskStatus";
import { Badge } from "../../components/Badge";
import { BrowserCrawlDialog } from "./BrowserCrawlDialog";
import { CrawlGraphView } from "./CrawlGraphView";
import { LiveTaskDialog } from "./LiveTaskDialog";
import { ResourcePoolsDialog } from "./ResourcePoolsDialog";
import { ScanDashboard } from "./ScanDashboard";
import { ScanDialog } from "./ScanDialog";
import { Button, IconButton } from "../../components/Button";
import { SplitPane } from "../../components/SplitPane";
import { EmptyState, ErrorState, LoadingState, Spinner } from "../../components/States";
import {
  formatBytes,
  formatClock,
  formatDateTime,
  formatDuration,
  statusTone,
} from "../../lib/format";
import { api, toIpcError } from "../../services/ipc";
import { openContextMenu } from "../../state/contextMenu";
import { exchangeMenu } from "../../state/tools";
import type { ScheduleRow } from "../../bindings/ScheduleRow";
import { useAppStore } from "../../state/app";
import { useTasksStore } from "../../state/tasks";

const STATUS_TONE: Record<TaskStatus, "ok" | "warn" | "danger" | "neutral" | "accent"> = {
  queued: "neutral",
  running: "accent",
  paused: "warn",
  completed: "ok",
  failed: "danger",
  cancelled: "neutral",
};

/** Tasks that keep going until they finish: crawls today. */
export function TasksView() {
  const load = useTasksStore((s) => s.load);
  useEffect(() => {
    void load();
  }, [load]);
  return (
    <div className="history-split">
      <SplitPane direction="vertical" initial={0.45} first={<TaskList />} second={<TaskPanel />} />
    </div>
  );
}

function configOf(task: TaskRow): CrawlConfig | null {
  try {
    return JSON.parse(task.config) as CrawlConfig;
  } catch {
    return null;
  }
}

/** Seconds ticking while a task runs. */
function useNow(active: boolean) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, [active]);
  return now;
}

function duration(task: TaskRow, now: number) {
  if (task.startedAt === null) return "—";
  return formatDuration((task.finishedAt ?? now) - task.startedAt);
}

function TaskControls({ task }: { task: TaskRow }) {
  const { pause, resume, cancel, remove } = useTasksStore.getState();
  if (task.status === "running" || task.status === "queued") {
    return (
      <>
        <IconButton icon="pause" label="Pause" onClick={() => void pause(task.id)} />
        <IconButton icon="stop" label="Stop" onClick={() => void cancel(task.id)} />
      </>
    );
  }
  if (task.status === "paused") {
    return (
      <>
        <IconButton icon="play" label="Resume" onClick={() => void resume(task.id)} />
        <IconButton icon="stop" label="Stop" onClick={() => void cancel(task.id)} />
      </>
    );
  }
  return (
    <IconButton
      icon="trash"
      label="Delete the task (its traffic stays)"
      onClick={() => void remove(task.id)}
    />
  );
}

function TaskList() {
  const rows = useTasksStore((s) => s.rows);
  const loaded = useTasksStore((s) => s.loaded);
  const error = useTasksStore((s) => s.error);
  const selectedId = useTasksStore((s) => s.selectedId);
  const { select, load, openCrawl, openDiscovery } = useTasksStore.getState();
  const [browserCrawl, setBrowserCrawl] = useState(false);
  const [scan, setScan] = useState(false);
  const [pools, setPools] = useState(false);
  const [live, setLive] = useState(false);
  const now = useNow(rows.some((r) => r.status === "running"));
  return (
    <div className="organizer-list">
      <div className="filters">
        <Button variant="ghost" icon="shield" onClick={() => setScan(true)}>
          New scan…
        </Button>
        <Button variant="ghost" icon="bolt" onClick={() => setLive(true)}>
          Live task…
        </Button>
        <Button variant="ghost" icon="nodes" onClick={() => setBrowserCrawl(true)}>
          Browser crawl…
        </Button>
        <Button variant="ghost" icon="search" onClick={() => void openDiscovery()}>
          Discover content
        </Button>
        <Button variant="primary" icon="spider" onClick={() => void openCrawl()}>
          New crawl
        </Button>
        <span className="muted">
          Scans, crawls, discovery and live tasks run in the background; pause, resume or stop them
          here.
        </span>
        <span className="spacer" />
        <Button variant="ghost" icon="layers" onClick={() => setPools(true)}>
          Resource pools…
        </Button>
        <span className="muted">
          {rows.length} {rows.length === 1 ? "task" : "tasks"}
        </span>
      </div>
      {pools && <ResourcePoolsDialog onClose={() => setPools(false)} />}
      {live && <LiveTaskDialog onClose={() => setLive(false)} />}
      {browserCrawl && <BrowserCrawlDialog onClose={() => setBrowserCrawl(false)} />}
      {scan && <ScanDialog onClose={() => setScan(false)} />}
      <SchedulesList />
      {error ? (
        <ErrorState title="Could not load tasks" message={error} onRetry={() => void load()} />
      ) : !loaded ? (
        <LoadingState />
      ) : rows.length === 0 ? (
        <EmptyState
          title="No tasks yet"
          action={
            <Button variant="primary" icon="spider" onClick={() => void openCrawl()}>
              New crawl
            </Button>
          }
        >
          A crawl follows the links, scripts and GET forms of a site to map it. Start one here, or
          with “Crawl from this URL” on any request (right-click in HTTP history).
        </EmptyState>
      ) : (
        <div className="endpoint-table-wrap">
          <table className="endpoint-table organizer-table">
            <thead>
              <tr>
                <th>Task</th>
                <th>Status</th>
                <th className="num">Requests</th>
                <th className="num">Found</th>
                <th className="num">Queued</th>
                <th className="num">Errors</th>
                <th>Started</th>
                <th>Duration</th>
                <th aria-label="Actions" />
              </tr>
            </thead>
            <tbody>
              {rows.map((t) => {
                const max = configOf(t)?.maxRequests;
                return (
                  <tr
                    key={t.id}
                    className={selectedId === t.id ? "selected" : undefined}
                    aria-selected={selectedId === t.id}
                    onClick={() => select(t.id)}
                  >
                    <td>{t.name}</td>
                    <td>
                      <Badge tone={STATUS_TONE[t.status]} title={t.lastError ?? undefined}>
                        {t.status === "running" && <Spinner label="Running" />} {t.status}
                      </Badge>
                    </td>
                    <td className="num">
                      {t.requests.toLocaleString()}
                      {max ? <span className="muted"> / {max.toLocaleString()}</span> : null}
                    </td>
                    <td className="num">{t.discovered.toLocaleString()}</td>
                    <td className="num">{t.queued.toLocaleString()}</td>
                    <td className="num">{t.errors.toLocaleString()}</td>
                    <td className="muted">{t.startedAt ? formatClock(t.startedAt) : "—"}</td>
                    <td className="muted">{duration(t, now)}</td>
                    <td onClick={(e) => e.stopPropagation()}>
                      <TaskControls task={t} />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

/** Task kinds whose requests go through resource pools. */
const POOLED_KINDS = ["crawl", "scan", "fuzz", "discovery", "apiSend"];

/** True while a task can be given its own resource pool. */
export function poolChoiceFor(task: Pick<TaskRow, "kind" | "status">): boolean {
  return (
    POOLED_KINDS.includes(task.kind) &&
    (task.status === "running" || task.status === "paused" || task.status === "queued")
  );
}

/** Chooses the resource pool of a running task (from its next request). */
function TaskPoolPicker({ task, pool }: { task: TaskRow; pool: string | null }) {
  const [names, setNames] = useState<string[]>([]);
  const notify = useAppStore((s) => s.notify);
  useEffect(() => {
    let alive = true;
    void api
      .resourcePools()
      .then((view) => alive && setNames(view.pools.map((p) => p.config.name)))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [task.id]);
  if (names.length === 0) return null;
  const current = names.find((n) => n.toLowerCase() === (pool ?? "").toLowerCase()) ?? "";
  return (
    <label className="inline-field" title="The resource pool this task sends through">
      Pool
      <select
        value={current}
        onChange={(e) => {
          void api
            .taskSetPool(task.id, e.target.value || null)
            .then(() => useTasksStore.getState().select(task.id))
            .catch((err: unknown) => notify("error", toIpcError(err).message));
        }}
      >
        <option value="">the tool&apos;s pool</option>
        {names.map((n) => (
          <option key={n} value={n}>
            {n}
          </option>
        ))}
      </select>
    </label>
  );
}

function TaskPanel() {
  const detail = useTasksStore((s) => s.detail);
  const { select } = useTasksStore.getState();
  if (detail.status === "idle") {
    return <EmptyState title="Select a task">Its settings and log appear here.</EmptyState>;
  }
  if (detail.status === "loading") return <LoadingState />;
  if (detail.status === "error") {
    return (
      <ErrorState
        title="Could not load the task"
        message={detail.message}
        onRetry={() => select(detail.id)}
      />
    );
  }
  const { row, logs, pool } = detail.detail;
  const config = row.kind === "crawl" ? configOf(row) : null;
  return (
    <div className="task-panel">
      <div className="exchange-summary">
        <strong>{row.name}</strong>
        <Badge tone={STATUS_TONE[row.status]}>{row.status}</Badge>
        <span className="muted">{row.progress}</span>
        {row.lastError && (
          <Badge tone="danger" title={row.lastError}>
            {row.lastError}
          </Badge>
        )}
        <span className="spacer" />
        <Button
          size="sm"
          variant="ghost"
          icon="logger"
          title="Crawler traffic is in the Logger (source: crawler)"
          onClick={() => useAppStore.getState().setView("logger")}
        >
          Traffic in Logger
        </Button>
        {poolChoiceFor(row) && <TaskPoolPicker task={row} pool={pool} />}
        <TaskControls task={row} />
      </div>
      <div className="task-body">
        {config && (
          <dl className="task-config">
            <dt>Start URLs</dt>
            <dd className="mono selectable">{config.startUrls.join("  ")}</dd>
            <dt>Limits</dt>
            <dd>
              depth {config.maxDepth} · {config.maxRequests.toLocaleString()} requests ·{" "}
              {config.concurrency} at a time ·{" "}
              {config.requestsPerSecond === 0 ? "no rate limit" : `${config.requestsPerSecond}/s`} ·
              timeout {config.timeoutSecs}s
            </dd>
            <dt>Options</dt>
            <dd>
              {config.submitGetForms ? "submits GET forms" : "no forms"} ·{" "}
              {config.useCookieJar ? "uses the cookie jar" : "no cookies"} · avoids{" "}
              {config.avoid.length} words
            </dd>
            <dt>Created</dt>
            <dd>{formatDateTime(row.createdAt)}</dd>
          </dl>
        )}
        {row.kind === "scan" && <ScanDashboard task={row} />}
        {(row.kind === "crawl" || row.kind === "scan") && <CrawlGraphView task={row} />}
        {row.kind === "discovery" && <DiscoveryResults task={row} />}
        {row.kind === "live" && isLiveCrawl(row) && <DiscoveryResults task={row} live />}
        <ol className="task-log" aria-label="Task log">
          {logs.map((l) => (
            <li key={l.id} className={`task-log-${l.level}`}>
              <span className="muted mono">{formatClock(l.at)}</span>
              <span className="selectable">{l.message}</span>
            </li>
          ))}
        </ol>
      </div>
    </div>
  );
}

/** A live passive crawl (its hits are URLs not requested yet). */
export function isLiveCrawl(task: TaskRow): boolean {
  try {
    return (JSON.parse(task.config) as { mode?: string }).mode === "crawl";
  } catch {
    return false;
  }
}

/** Hits of a content discovery task or a live crawl (reloaded as it finds
 * more). For a live crawl the exchange is the page the URL was found on and
 * status 0 means nobody requested it yet. */
function DiscoveryResults({ task, live = false }: { task: TaskRow; live?: boolean }) {
  const [hits, setHits] = useState<DiscoveryHit[] | null>(null);
  useEffect(() => {
    let live = true;
    api
      .discoveryResults(task.id)
      .then((h) => live && setHits(h))
      .catch(() => live && setHits([]));
    return () => {
      live = false;
    };
  }, [task.id, task.discovered, task.progress]);
  if (hits === null) return <LoadingState />;
  if (hits.length === 0) return <p className="muted">Nothing found yet.</p>;
  return (
    <div className="endpoint-table-wrap discovery-results">
      <table className="endpoint-table organizer-table" aria-label="Discovered content">
        <thead>
          <tr>
            <th>Found</th>
            <th>Kind</th>
            <th>{live ? "Requested" : "Status"}</th>
            <th className="num">Length</th>
            <th>URL</th>
          </tr>
        </thead>
        <tbody>
          {hits.map((h) => (
            <tr
              key={h.id}
              onContextMenu={(e) =>
                h.exchangeId !== null &&
                openContextMenu(
                  e,
                  "Request actions",
                  exchangeMenu({ id: h.exchangeId, url: h.url }),
                )
              }
            >
              <td className="num">{formatClock(h.foundAt)}</td>
              <td>{h.kind}</td>
              <td>
                {live && h.status === 0 ? (
                  <span className="muted">not yet</span>
                ) : (
                  <span className={`status status-${statusTone(h.status)}`}>{h.status}</span>
                )}
              </td>
              <td className="num">{formatBytes(h.length)}</td>
              <td className="mono selectable">{h.url}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

const REPEAT_LABEL: Record<number, string> = {
  0: "once",
  60: "hourly",
  1440: "daily",
  10080: "weekly",
};

/** Scheduled crawls and discovery (they run only while this project is open). */
function SchedulesList() {
  const rows = useTasksStore((s) => s.rows);
  const [schedules, setSchedules] = useState<ScheduleRow[]>([]);
  const reload = () =>
    void api
      .schedules()
      .then(setSchedules)
      .catch(() => setSchedules([]));
  useEffect(reload, [rows]);
  if (schedules.length === 0) return null;
  return (
    <section className="schedules" aria-label="Schedules">
      <strong>Scheduled</strong>
      <span className="muted"> — they run only while this project is open in ChosProxy.</span>
      <ul>
        {schedules.map((s) => (
          <li key={s.id} className={s.enabled ? undefined : "muted"}>
            <span className="mono">{s.kind}</span>
            <span>{s.name}</span>
            <span className="muted">
              {s.enabled
                ? `next ${new Date(s.nextRun).toLocaleString()}`
                : s.lastRun
                  ? `last ${new Date(s.lastRun).toLocaleString()}`
                  : "off"}{" "}
              · {REPEAT_LABEL[s.repeatMinutes] ?? `every ${s.repeatMinutes} min`}
            </span>
            {s.lastError && (
              <Badge tone="danger" title={s.lastError}>
                last run failed
              </Badge>
            )}
            <span className="spacer" />
            <Button
              size="sm"
              variant="ghost"
              onClick={() => void api.scheduleSetEnabled(s.id, !s.enabled).then(reload)}
            >
              {s.enabled ? "Turn off" : "Turn on"}
            </Button>
            <IconButton
              icon="trash"
              label={`Delete schedule ${s.name}`}
              onClick={() => void api.scheduleDelete(s.id).then(reload)}
            />
          </li>
        ))}
      </ul>
    </section>
  );
}
