import { create } from "zustand";
import type { ExchangeDetail } from "../bindings/ExchangeDetail";
import type { SearchQuery } from "../bindings/SearchQuery";
import type { SearchResults } from "../bindings/SearchResults";
import { DEFAULT_FILTER, toFilterQuery } from "../lib/filters";
import { api, toIpcError } from "../services/ipc";

export const DEFAULT_SEARCH: SearchQuery = {
  text: "",
  regex: false,
  caseSensitive: false,
  inUrls: true,
  inRequestHeaders: true,
  inRequestBodies: true,
  inResponseHeaders: true,
  inResponseBodies: true,
  inNotes: true,
  inWebsockets: true,
  filter: toFilterQuery(DEFAULT_FILTER),
  maxHits: 500,
};

type Detail =
  | { status: "idle" }
  | { status: "loading"; id: number }
  | { status: "ready"; id: number; detail: ExchangeDetail }
  | { status: "error"; id: number; message: string };

interface SearchState {
  query: SearchQuery;
  results: SearchResults | null;
  running: boolean;
  error: string | null;
  /** Index of the selected hit. */
  selected: number | null;
  detail: Detail;

  setQuery(patch: Partial<SearchQuery>): void;
  setInScopeOnly(on: boolean): void;
  run(): Promise<void>;
  select(index: number | null): void;
  reset(): void;
}

let seq = 0;

export const useSearchStore = create<SearchState>((set, get) => ({
  query: DEFAULT_SEARCH,
  results: null,
  running: false,
  error: null,
  selected: null,
  detail: { status: "idle" },

  setQuery: (patch) => set((s) => ({ query: { ...s.query, ...patch } })),

  setInScopeOnly: (on) =>
    set((s) => ({ query: { ...s.query, filter: { ...s.query.filter, inScopeOnly: on } } })),

  async run() {
    const mine = ++seq;
    set({ running: true, error: null, selected: null, detail: { status: "idle" } });
    try {
      const results = await api.search(get().query);
      // A newer search replaced this one; keep its results instead.
      if (mine !== seq || results.superseded) return;
      set({ results });
    } catch (e) {
      if (mine === seq) set({ error: toIpcError(e).message, results: null });
    } finally {
      if (mine === seq) set({ running: false });
    }
  },

  select(index) {
    const hit = index === null ? undefined : get().results?.hits[index];
    if (index === null || !hit) {
      set({ selected: null, detail: { status: "idle" } });
      return;
    }
    const id = hit.row.id;
    set({ selected: index, detail: { status: "loading", id } });
    api
      .exchangeDetail(id)
      .then((detail) => {
        if (get().selected === index) set({ detail: { status: "ready", id, detail } });
      })
      .catch((e: unknown) => {
        if (get().selected === index) {
          set({ detail: { status: "error", id, message: toIpcError(e).message } });
        }
      });
  },

  reset() {
    seq++;
    set({ results: null, running: false, error: null, selected: null, detail: { status: "idle" } });
  },
}));
