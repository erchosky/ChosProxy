import { create } from "zustand";
import type { SiteMap } from "../bindings/SiteMap";
import type { SiteMark } from "../bindings/SiteMark";
import { type Conditions, conditionsScript } from "../lib/conditions";
import { DEFAULT_FILTER, type HistoryFilter, toFilterQuery } from "../lib/filters";
import { type SiteNode, buildSiteTree } from "../lib/siteTree";
import { api, toIpcError } from "../services/ipc";
import { useAppStore } from "./app";

/** Filters offered by the site map (a subset of the history filter). */
export interface SiteMapFilter {
  search: string;
  inScopeOnly: boolean;
  hideStatic: boolean;
}

interface SiteMapState {
  filter: SiteMapFilter;
  map: SiteMap | null;
  tree: SiteNode[];
  loading: boolean;
  error: string | null;
  selectedKey: string | null;
  expanded: Record<string, boolean>;
  /** Conditions from the condition builder (M31b): only the endpoints of
   * requests that meet them are shown. */
  conditions: Conditions | null;
  /** What the conditions looked at, when they are applied. */
  scriptInfo: {
    scanned: number;
    capped: boolean;
    errors: number;
    firstError: string | null;
  } | null;

  load(): Promise<void>;
  setFilter(patch: Partial<SiteMapFilter>): void;
  setConditions(conditions: Conditions | null): void;
  select(key: string | null): void;
  toggle(key: string): void;
  setMark(node: SiteNode, mark: SiteMark | null): Promise<void>;
  reset(): void;
}

function query(f: SiteMapFilter) {
  const filter: HistoryFilter = {
    ...DEFAULT_FILTER,
    search: f.search,
    inScopeOnly: f.inScopeOnly,
    hideStatic: f.hideStatic,
  };
  return toFilterQuery(filter);
}

let seq = 0;

export const useSiteMapStore = create<SiteMapState>((set, get) => ({
  filter: { search: "", inScopeOnly: false, hideStatic: false },
  map: null,
  tree: [],
  loading: false,
  error: null,
  selectedKey: null,
  expanded: {},
  conditions: null,
  scriptInfo: null,

  async load() {
    const mine = ++seq;
    set({ loading: true });
    try {
      const conditions = get().conditions;
      let map: SiteMap;
      let scriptInfo: SiteMapState["scriptInfo"] = null;
      if (conditions) {
        const scripted = await api.siteMapConditions(
          query(get().filter),
          conditionsScript(conditions),
        );
        map = scripted.map;
        scriptInfo = {
          scanned: scripted.scanned,
          capped: scripted.capped,
          errors: scripted.errors,
          firstError: scripted.firstError,
        };
      } else {
        map = await api.siteMap(query(get().filter));
      }
      if (mine !== seq) return;
      set({ map, tree: buildSiteTree(map.entries, map.marks), error: null, scriptInfo });
    } catch (e) {
      if (mine !== seq) return;
      const err = toIpcError(e);
      set({ error: err.kind === "noProject" ? null : err.message });
    } finally {
      if (mine === seq) set({ loading: false });
    }
  },

  setFilter(patch) {
    set((s) => ({ filter: { ...s.filter, ...patch } }));
    void get().load();
  },

  setConditions(conditions) {
    set({ conditions });
    void get().load();
  },

  select: (selectedKey) => set({ selectedKey }),

  toggle: (key) => set((s) => ({ expanded: { ...s.expanded, [key]: !s.expanded[key] } })),

  async setMark(node, mark) {
    try {
      await api.setSiteMark(
        { scheme: node.scheme, host: node.host, port: node.port, path: node.path },
        mark,
      );
      await get().load();
    } catch (e) {
      useAppStore.getState().notify("error", `Could not mark the node: ${toIpcError(e).message}`);
    }
  },

  reset() {
    seq++;
    set({
      map: null,
      tree: [],
      selectedKey: null,
      expanded: {},
      error: null,
      loading: false,
      scriptInfo: null,
    });
  },
}));
