// Tasks (crawls today): list, detail with log, controls, and the "New
// crawl" dialog shared by every view that can start one.
import { create } from "zustand";
import type { CrawlConfig } from "../bindings/CrawlConfig";
import type { DiscoveryConfig } from "../bindings/DiscoveryConfig";
import type { TaskDetail } from "../bindings/TaskDetail";
import type { TaskRow } from "../bindings/TaskRow";
import { api, toIpcError } from "../services/ipc";
import { useAppStore } from "./app";

type Detail =
  | { status: "idle" }
  | { status: "loading"; id: number }
  | { status: "ready"; id: number; detail: TaskDetail }
  | { status: "error"; id: number; message: string };

interface TasksState {
  rows: TaskRow[];
  loaded: boolean;
  error: string | null;
  selectedId: number | null;
  detail: Detail;
  /** Settings in the "New crawl" dialog (null when closed). */
  crawl: CrawlConfig | null;
  /** Settings in the "Discover content" dialog (null when closed). */
  discovery: DiscoveryConfig | null;

  load(): Promise<void>;
  select(id: number | null): void;
  /** Reloads the list and the selected task (after a change). */
  refresh(): Promise<void>;
  pause(id: number): Promise<void>;
  resume(id: number): Promise<void>;
  cancel(id: number): Promise<void>;
  remove(id: number): Promise<void>;
  /** Opens the "New crawl" dialog, optionally with start URLs. */
  openCrawl(startUrls?: string[]): Promise<void>;
  editCrawl(patch: Partial<CrawlConfig>): void;
  closeCrawl(): void;
  /** Starts the crawl in the dialog; resolves to an error message or null. */
  startCrawl(pool?: string): Promise<string | null>;
  /** Opens the "Discover content" dialog, optionally under a folder URL. */
  openDiscovery(baseUrl?: string): Promise<void>;
  editDiscovery(patch: Partial<DiscoveryConfig>): void;
  closeDiscovery(): void;
  /** Starts the discovery in the dialog; resolves to an error message or null. */
  startDiscovery(pool?: string): Promise<string | null>;
  reset(): void;
}

let seq = 0;

/** Sites proxied recently (in scope when a scope is defined), as start URLs. */
export async function suggestedStarts(): Promise<string[]> {
  try {
    const page = await api.historyPage({
      offset: 0,
      limit: 50,
      order: "desc",
      sort: "id",
      search: null,
      searchRegex: false,
      inScopeOnly: false,
      mimes: [],
      sources: ["proxy"],
      highlights: [],
      annotatedOnly: false,
      site: null,
      methods: [],
      statusClasses: [],
      hideExtensions: [],
    });
    const rows = page.rows.some((r) => r.inScope) ? page.rows.filter((r) => r.inScope) : page.rows;
    const origins: string[] = [];
    for (const r of rows) {
      const standard =
        (r.scheme === "https" && r.port === 443) || (r.scheme === "http" && r.port === 80);
      const host = r.host.includes(":") ? `[${r.host}]` : r.host;
      const origin = `${r.scheme}://${host}${standard ? "" : `:${r.port}`}/`;
      if (!origins.includes(origin)) origins.push(origin);
      if (origins.length === 3) break;
    }
    return origins;
  } catch {
    return [];
  }
}

export const useTasksStore = create<TasksState>((set, get) => {
  const act = async (label: string, run: () => Promise<unknown>) => {
    try {
      await run();
      await get().refresh();
    } catch (e) {
      useAppStore.getState().notify("error", `${label}: ${toIpcError(e).message}`);
    }
  };

  return {
    rows: [],
    loaded: false,
    error: null,
    selectedId: null,
    detail: { status: "idle" },
    crawl: null,
    discovery: null,

    async load() {
      const mine = ++seq;
      try {
        const rows = await api.tasks();
        if (mine === seq) set({ rows, loaded: true, error: null });
      } catch (e) {
        const err = toIpcError(e);
        if (mine === seq) {
          set({ loaded: true, error: err.kind === "noProject" ? null : err.message });
        }
      }
    },

    select(id) {
      set({ selectedId: id });
      if (id === null) {
        set({ detail: { status: "idle" } });
        return;
      }
      const current = get().detail;
      if (current.status === "idle" || current.id !== id)
        set({ detail: { status: "loading", id } });
      api
        .taskDetail(id)
        .then((detail) => {
          if (get().selectedId === id) set({ detail: { status: "ready", id, detail } });
        })
        .catch((e: unknown) => {
          if (get().selectedId === id) {
            set({ detail: { status: "error", id, message: toIpcError(e).message } });
          }
        });
    },

    async refresh() {
      await get().load();
      const id = get().selectedId;
      if (id !== null) {
        if (get().rows.some((r) => r.id === id)) get().select(id);
        else get().select(null);
      }
    },

    pause: (id) => act("Could not pause", () => api.taskPause(id)),
    resume: (id) => act("Could not resume", () => api.taskResume(id)),
    cancel: (id) => act("Could not stop", () => api.taskCancel(id)),
    remove: (id) => act("Could not delete", () => api.taskDelete(id)),

    async openCrawl(startUrls) {
      try {
        const defaults = await api.crawlDefaults();
        const urls = startUrls ?? (await suggestedStarts());
        set({ crawl: { ...defaults, startUrls: urls } });
      } catch (e) {
        useAppStore
          .getState()
          .notify("error", `Could not open the crawl settings: ${toIpcError(e).message}`);
      }
    },

    editCrawl(patch) {
      const crawl = get().crawl;
      if (crawl) set({ crawl: { ...crawl, ...patch } });
    },

    closeCrawl: () => set({ crawl: null }),

    async startCrawl(pool = "") {
      const crawl = get().crawl;
      if (!crawl) return null;
      try {
        const task = await api.crawlStart(crawl, pool || null);
        set({ crawl: null, selectedId: task.id });
        useAppStore.getState().notify("info", `${task.name} started`);
        useAppStore.getState().setView("tasks");
        await get().refresh();
        return null;
      } catch (e) {
        return toIpcError(e).message;
      }
    },

    async openDiscovery(baseUrl) {
      try {
        const defaults = await api.discoveryDefaults();
        const suggested = baseUrl ?? (await suggestedStarts())[0] ?? "";
        set({ discovery: { ...defaults, baseUrl: suggested } });
      } catch (e) {
        useAppStore
          .getState()
          .notify("error", `Could not open the discovery settings: ${toIpcError(e).message}`);
      }
    },

    editDiscovery(patch) {
      const discovery = get().discovery;
      if (discovery) set({ discovery: { ...discovery, ...patch } });
    },

    closeDiscovery: () => set({ discovery: null }),

    async startDiscovery(pool = "") {
      const discovery = get().discovery;
      if (!discovery) return null;
      try {
        const task = await api.discoveryStart(discovery, pool || null);
        set({ discovery: null, selectedId: task.id });
        useAppStore.getState().notify("info", `${task.name} started`);
        useAppStore.getState().setView("tasks");
        await get().refresh();
        return null;
      } catch (e) {
        return toIpcError(e).message;
      }
    },

    reset: () =>
      set({
        rows: [],
        loaded: false,
        error: null,
        selectedId: null,
        detail: { status: "idle" },
        crawl: null,
        discovery: null,
      }),
  };
});
