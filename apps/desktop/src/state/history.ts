import { create, type StoreApi, type UseBoundStore } from "zustand";
import type { Source } from "../bindings/Source";
import type { ExchangeDetail } from "../bindings/ExchangeDetail";
import type { HistoryRow } from "../bindings/HistoryRow";
import { type Conditions, conditionsScript } from "../lib/conditions";
import { DEFAULT_FILTER, type HistoryFilter, toQuery } from "../lib/filters";
import { api, toIpcError } from "../services/ipc";

/** Rows fetched per request. */
export const PAGE_SIZE = 200;
/** Pages kept on each side of the visible ones; farther pages are dropped. */
export const KEEP_PAGES = 5;

/** Pages within `KEEP_PAGES` of the visible range `[first, last]`. */
export function prunePages<T>(
  pages: Record<number, T>,
  first: number,
  last: number,
): Record<number, T> {
  const out: Record<number, T> = {};
  for (const [k, v] of Object.entries(pages)) {
    const p = Number(k);
    if (p >= first - KEEP_PAGES && p <= last + KEEP_PAGES) out[p] = v;
  }
  return out;
}

type DetailState =
  | { status: "idle" }
  | { status: "loading"; id: number }
  | { status: "ready"; id: number; detail: ExchangeDetail }
  | { status: "error"; id: number; message: string };

export interface HistoryState {
  filter: HistoryFilter;
  /** Tools this list is limited to (HTTP history: the proxy); null = all. */
  fixedSources: Source[] | null;
  /** Keep the newest row in view as traffic arrives. */
  follow: boolean;
  total: number;
  /** Loaded pages by page index. */
  pages: Record<number, HistoryRow[]>;
  /** Bumped whenever cached pages become invalid. */
  generation: number;
  loading: boolean;
  error: string | null;
  selectedId: number | null;
  detail: DetailState;
  /** Filter script applied on top of the filter (ChosScripts). */
  scriptFilter: string | null;
  /** Conditions from the condition builder (run as a filter script). */
  conditions: Conditions | null;
  /** What the filter script looked at. */
  scriptInfo: {
    scanned: number;
    capped: boolean;
    errors: number;
    firstError: string | null;
  } | null;
  /** Column script, and its values by exchange id. */
  scriptColumn: string | null;
  columnValues: Record<number, string>;

  setFilter(patch: Partial<HistoryFilter>): void;
  setScriptFilter(id: string | null): void;
  setConditions(conditions: Conditions | null): void;
  setScriptColumn(id: string | null): void;
  setFollow(follow: boolean): void;
  /** Ensures the pages covering rows [start, end] are loaded. */
  ensureRange(start: number, end: number): void;
  /** Drops cached pages and reloads (e.g. after new traffic). */
  invalidate(): void;
  /** Forgets everything (another project was opened) and reloads. */
  reset(): void;
  select(id: number | null): void;
  rowAt(index: number): HistoryRow | undefined;
  /** Re-reads the selected exchange (e.g. after editing its annotation). */
  refreshDetail(): void;
  /** Index of a loaded row by id, or null if it is not in a loaded page. */
  indexOfId(id: number): number | null;
}

export type HistoryStore = UseBoundStore<StoreApi<HistoryState>>;

interface HistoryOptions {
  filter: HistoryFilter;
  fixedSources: Source[] | null;
}

/** A paged, filtered view of stored traffic (HTTP history, Logger). */
export function createHistoryStore(options: HistoryOptions): HistoryStore {
  const inFlight = new Set<string>();
  // Pages on screen, from the last ensureRange call.
  let visible: [number, number] = [0, 0];
  return create<HistoryState>((set, get) => {
    async function loadPage(page: number) {
      const { filter, generation } = get();
      const key = `${generation}:${page}`;
      if (inFlight.has(key)) return;
      inFlight.add(key);
      set({ loading: true });
      try {
        const query = toQuery(filter, page * PAGE_SIZE, PAGE_SIZE);
        if (options.fixedSources) query.sources = options.fixedSources;
        const script = get().scriptFilter;
        const conditions = get().conditions;
        let result;
        if (script || conditions) {
          const scripted = conditions
            ? await api.historyPageConditions(query, conditionsScript(conditions))
            : await api.historyPageScripted(query, script ?? "");
          result = scripted.page;
          if (get().generation !== generation) return;
          set({
            scriptInfo: {
              scanned: scripted.scanned,
              capped: scripted.capped,
              errors: scripted.errors,
              firstError: scripted.firstError,
            },
          });
        } else {
          result = await api.historyPage(query);
        }
        if (get().generation !== generation) return; // stale response
        set((s) => ({
          total: result.total,
          pages: prunePages({ ...s.pages, [page]: result.rows }, visible[0], visible[1]),
          error: null,
        }));
        void loadColumn(result.rows.map((r) => r.id));
      } catch (e) {
        const err = toIpcError(e);
        set({ error: err.kind === "noProject" ? null : err.message });
      } finally {
        inFlight.delete(key);
        set({ loading: inFlight.size > 0 });
      }
    }

    async function loadColumn(ids: number[]) {
      const column = get().scriptColumn;
      if (!column) return;
      const missing = ids.filter((id) => !(id in get().columnValues));
      if (missing.length === 0) return;
      try {
        const cells = await api.scriptColumn(column, missing);
        if (get().scriptColumn !== column) return;
        set((s) => {
          const values = { ...s.columnValues };
          for (const c of cells) values[c.id] = c.error ? `⚠ ${c.error}` : c.value;
          return { columnValues: values };
        });
      } catch (e) {
        set({ scriptColumn: null, columnValues: {}, error: toIpcError(e).message });
      }
    }

    return {
      filter: options.filter,
      scriptFilter: null,
      conditions: null,
      scriptInfo: null,
      scriptColumn: null,
      columnValues: {},
      fixedSources: options.fixedSources,
      follow: false,
      total: 0,
      pages: {},
      generation: 0,
      loading: false,
      error: null,
      selectedId: null,
      detail: { status: "idle" },

      setFilter(patch) {
        set((s) => ({
          filter: { ...s.filter, ...patch },
          pages: {},
          generation: s.generation + 1,
        }));
        void loadPage(0);
      },

      setConditions(conditions) {
        set((s) => ({
          conditions: conditions && conditions.items.length > 0 ? conditions : null,
          scriptFilter: null,
          scriptInfo: null,
          pages: {},
          generation: s.generation + 1,
        }));
        void loadPage(0);
      },

      setScriptFilter(id) {
        set((s) => ({
          conditions: null,
          scriptFilter: id,
          scriptInfo: null,
          pages: {},
          generation: s.generation + 1,
        }));
        void loadPage(0);
      },

      setScriptColumn(id) {
        set({ scriptColumn: id, columnValues: {} });
        const ids = Object.values(get().pages).flatMap((rows) => rows.map((r) => r.id));
        void loadColumn(ids);
      },

      setFollow: (follow) => set({ follow }),

      ensureRange(start, end) {
        const first = Math.floor(Math.max(0, start) / PAGE_SIZE);
        const last = Math.floor(Math.max(0, end) / PAGE_SIZE);
        visible = [first, last];
        for (let p = first; p <= last; p++) {
          if (!get().pages[p]) void loadPage(p);
        }
      },

      invalidate() {
        // Keep showing the current rows until fresh ones arrive; reload only
        // the visible pages (page 0 also refreshes the total).
        set((s) => ({ generation: s.generation + 1 }));
        const pages = new Set([0]);
        for (let p = visible[0]; p <= visible[1]; p++) pages.add(p);
        for (const p of pages) void loadPage(p);
      },

      reset() {
        set((s) => ({
          total: 0,
          pages: {},
          generation: s.generation + 1,
          selectedId: null,
          detail: { status: "idle" },
          error: null,
        }));
        void loadPage(0);
      },

      select(id) {
        set({ selectedId: id });
        if (id === null) {
          set({ detail: { status: "idle" } });
          return;
        }
        set({ detail: { status: "loading", id } });
        api
          .exchangeDetail(id)
          .then((detail) => {
            if (get().selectedId === id) set({ detail: { status: "ready", id, detail } });
          })
          .catch((e: unknown) => {
            if (get().selectedId === id) {
              set({ detail: { status: "error", id, message: toIpcError(e).message } });
            }
          });
      },

      refreshDetail() {
        const id = get().selectedId;
        if (id === null) return;
        api
          .exchangeDetail(id)
          .then((detail) => {
            if (get().selectedId === id) set({ detail: { status: "ready", id, detail } });
          })
          .catch(() => {
            // Keep showing the previous detail; the next selection retries.
          });
      },

      rowAt(index) {
        return get().pages[Math.floor(index / PAGE_SIZE)]?.[index % PAGE_SIZE];
      },

      indexOfId(id) {
        for (const [page, rows] of Object.entries(get().pages)) {
          const i = rows.findIndex((r) => r.id === id);
          if (i !== -1) return Number(page) * PAGE_SIZE + i;
        }
        return null;
      },
    };
  });
}

/** Proxy → HTTP history: traffic relayed by the proxy listeners. */
export const useHistoryStore = createHistoryStore({
  filter: DEFAULT_FILTER,
  fixedSources: ["proxy"],
});

/** Logger: every tool's traffic, oldest first. */
export const useLoggerStore = createHistoryStore({
  filter: { ...DEFAULT_FILTER, newestFirst: false },
  fixedSources: null,
});

/** Every traffic list, e.g. to refresh them all after new traffic. */
export const allHistoryStores: readonly HistoryStore[] = [useHistoryStore, useLoggerStore];
