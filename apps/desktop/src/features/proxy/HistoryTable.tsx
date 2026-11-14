import { useVirtualizer } from "@tanstack/react-virtual";
import { Fragment, type KeyboardEvent, useEffect, useRef } from "react";
import type { HistoryRow } from "../../bindings/HistoryRow";
import type { HistorySort } from "../../bindings/HistorySort";
import { Button } from "../../components/Button";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { clearedFilter, isFiltered } from "../../lib/filters";
import { formatBytes, formatClock, formatDuration, statusTone } from "../../lib/format";
import { isMenuKey, openContextMenu, openMenuBelow } from "../../state/contextMenu";
import { useScriptsStore } from "../../state/scripts";
import { exchangeMenu } from "../../state/tools";
import { useHistoryCtx } from "./historyContext";

const ROW_HEIGHT = 26;

export const COLUMNS = [
  { key: "scope", label: "", width: "14px" },
  { key: "id", label: "#", width: "64px" },
  { key: "time", label: "Time", width: "96px" },
  { key: "method", label: "Method", width: "70px" },
  { key: "host", label: "Host", width: "minmax(120px, 1.2fr)" },
  { key: "path", label: "Path", width: "minmax(160px, 3fr)" },
  { key: "status", label: "Status", width: "60px" },
  { key: "length", label: "Length", width: "76px" },
  { key: "mime", label: "MIME", width: "70px" },
  { key: "protocol", label: "Protocol", width: "74px" },
  { key: "tls", label: "TLS", width: "44px" },
  { key: "duration", label: "Time taken", width: "80px" },
  { key: "source", label: "Source", width: "66px" },
  { key: "notes", label: "Notes", width: "minmax(80px, 1fr)" },
] as const;

const GRID = COLUMNS.map((c) => c.width).join(" ");
/** With a script column, it goes before Notes. */
const GRID_WITH_SCRIPT = [
  ...COLUMNS.slice(0, -1).map((c) => c.width),
  "minmax(100px, 1.2fr)",
  "minmax(80px, 1fr)",
].join(" ");

/** Columns the history can be sorted by. */
const SORTABLE: Partial<Record<(typeof COLUMNS)[number]["key"], HistorySort>> = {
  id: "id",
  method: "method",
  host: "host",
  path: "path",
  status: "status",
  length: "length",
  mime: "mime",
  duration: "duration",
};

function Row({ row, extra }: { row: HistoryRow; extra?: string | null }) {
  const tone = statusTone(row.status);
  const flags = [
    row.error ? "error" : null,
    row.clTeConflict ? "CL/TE conflict" : null,
    row.responseTruncated || row.requestTruncated ? "truncated" : null,
    row.upgraded ? "upgraded" : null,
    row.edited ? "edited" : null,
    row.dropped ? "dropped" : null,
  ].filter(Boolean);
  return (
    <>
      <span
        className={`scope-dot${row.inScope ? " in" : ""}`}
        title={row.inScope ? "In scope" : "Out of scope"}
      />
      <span className="muted">{row.id}</span>
      <span className="muted">{formatClock(row.startedAt)}</span>
      <span className={`method method-${row.method.toLowerCase()}`}>{row.method}</span>
      <span title={`${row.host}:${row.port}`}>{row.host}</span>
      <span title={row.url}>{row.path}</span>
      <span className={`status status-${tone}`} title={row.error ?? undefined}>
        {row.status ?? (row.error ? "ERR" : "—")}
      </span>
      <span className="num">{formatBytes(row.responseLen)}</span>
      <span>{row.mime ?? ""}</span>
      <span className="muted">{row.httpVersion}</span>
      <span className="muted">{row.tls ? "✓" : ""}</span>
      <span className="num">{formatDuration(row.durationMs)}</span>
      <span className="muted">{row.source}</span>
      {extra !== undefined && (
        <span className="script-cell" title={extra ?? undefined}>
          {extra ?? "…"}
        </span>
      )}
      <span className="muted" title={[row.error, row.notes].filter(Boolean).join("\n")}>
        {flags.join(", ") || row.notes || ""}
      </span>
    </>
  );
}

export function HistoryTable() {
  const useStore = useHistoryCtx();
  const total = useStore((s) => s.total);
  const pages = useStore((s) => s.pages);
  const loading = useStore((s) => s.loading);
  const error = useStore((s) => s.error);
  const filter = useStore((s) => s.filter);
  const selectedId = useStore((s) => s.selectedId);
  const follow = useStore((s) => s.follow);
  const scriptColumn = useStore((s) => s.scriptColumn);
  const columnValues = useStore((s) => s.columnValues);
  const columnName = useScriptsStore(
    (s) => s.library?.config.scripts.find((x) => x.id === scriptColumn)?.name ?? scriptColumn,
  );
  const grid = scriptColumn ? GRID_WITH_SCRIPT : GRID;
  const { ensureRange, invalidate, select, rowAt, indexOfId, setFilter } = useStore.getState();
  const parent = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: total,
    getScrollElement: () => parent.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 30,
  });
  const items = virtualizer.getVirtualItems();
  const first = items[0]?.index ?? 0;
  const last = items[items.length - 1]?.index ?? 0;

  useEffect(() => {
    invalidate();
  }, [invalidate]);

  useEffect(() => {
    if (total > 0) ensureRange(first, last);
  }, [first, last, total, pages, ensureRange]);

  // Follow mode: keep the newest row in view as traffic arrives.
  useEffect(() => {
    if (!follow || total === 0) return;
    if (filter.newestFirst) virtualizer.scrollToIndex(0, { align: "start" });
    else virtualizer.scrollToIndex(total - 1, { align: "end" });
  }, [follow, total, filter.newestFirst, virtualizer]);

  function selectIndex(index: number) {
    const clamped = Math.max(0, Math.min(total - 1, index));
    const row = rowAt(clamped);
    virtualizer.scrollToIndex(clamped);
    if (row) select(row.id);
  }

  function onKeyDown(e: KeyboardEvent) {
    if (total === 0) return;
    if (isMenuKey(e)) {
      const index = selectedId === null ? undefined : indexOfId(selectedId);
      const row = index === undefined || index === null ? undefined : rowAt(index);
      const el = parent.current?.querySelector('[role="row"][aria-selected="true"]');
      if (row && el) {
        e.preventDefault();
        openMenuBelow(el, "Request actions", exchangeMenu(row));
      }
      return;
    }
    // Resolve the selection by id: new traffic shifts row positions.
    const current = selectedId === null ? -1 : (indexOfId(selectedId) ?? -1);
    const pageRows = Math.max(
      1,
      Math.floor((parent.current?.clientHeight ?? 260) / ROW_HEIGHT) - 1,
    );
    const moves: Record<string, number> = {
      ArrowDown: current + 1,
      ArrowUp: current - 1,
      PageDown: current + pageRows,
      PageUp: current - pageRows,
      Home: 0,
      End: total - 1,
    };
    const target = moves[e.key];
    if (target === undefined) return;
    e.preventDefault();
    selectIndex(target);
  }

  if (error)
    return <ErrorState title="Could not load history" message={error} onRetry={invalidate} />;

  return (
    <div className="history-table">
      <div className="table-header" style={{ gridTemplateColumns: grid }} role="row">
        {COLUMNS.map((c) => {
          const sort = SORTABLE[c.key];
          const scriptHeader =
            scriptColumn && c.key === "notes" ? (
              <span key="script" role="columnheader" title="Column script">
                {columnName}
              </span>
            ) : null;
          if (!sort)
            return (
              <Fragment key={c.key}>
                {scriptHeader}
                <span role="columnheader">{c.label}</span>
              </Fragment>
            );
          const active = filter.sort === sort;
          return (
            <span
              key={c.key}
              role="columnheader"
              aria-sort={active ? (filter.newestFirst ? "descending" : "ascending") : "none"}
            >
              <button
                type="button"
                className="column-sort"
                title={`Sort by ${c.label === "#" ? "capture order" : c.label}`}
                onClick={() =>
                  setFilter(
                    active
                      ? { newestFirst: !filter.newestFirst }
                      : { sort, newestFirst: sort === "id" || sort === "length" },
                  )
                }
              >
                {c.label}
                {active && <span aria-hidden="true">{filter.newestFirst ? " ↓" : " ↑"}</span>}
              </button>
            </span>
          );
        })}
      </div>
      {total === 0 ? (
        loading ? (
          <LoadingState />
        ) : isFiltered(filter) ? (
          <EmptyState
            title="No requests match the filters"
            action={
              <Button size="sm" onClick={() => setFilter(clearedFilter(filter))}>
                Clear filters
              </Button>
            }
          />
        ) : (
          <EmptyState title="No requests captured yet">
            Configure your test browser to use the proxy listener as its HTTP proxy, then browse the
            target.
          </EmptyState>
        )
      ) : (
        <div
          className="table-body"
          ref={parent}
          tabIndex={0}
          role="grid"
          aria-label="HTTP history"
          aria-rowcount={total}
          onKeyDown={onKeyDown}
        >
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {items.map((item) => {
              const row = rowAt(item.index);
              const selected = row !== undefined && row.id === selectedId;
              return (
                <div
                  key={item.key}
                  role="row"
                  aria-selected={selected}
                  className={`table-row${selected ? " selected" : ""}${row?.error ? " has-error" : ""}${row?.highlight ? ` hl-${row.highlight}` : ""}`}
                  style={{
                    gridTemplateColumns: grid,
                    transform: `translateY(${item.start}px)`,
                    height: ROW_HEIGHT,
                  }}
                  onClick={() => {
                    if (row) select(row.id);
                  }}
                  onContextMenu={(e) => {
                    if (!row) return;
                    select(row.id);
                    openContextMenu(e, "Request actions", exchangeMenu(row));
                  }}
                >
                  {row ? (
                    <Row
                      row={row}
                      extra={scriptColumn ? (columnValues[row.id] ?? null) : undefined}
                    />
                  ) : (
                    <span className="muted">Loading…</span>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
