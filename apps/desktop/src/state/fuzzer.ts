// Fuzzer: the attack being set up (request with § positions, payload sets,
// grep, options), the attacks sent (tasks of kind "fuzz") and their results.
import type { PointKind } from "../bindings/PointKind";
import { create } from "zustand";
import type { ExchangeDetail } from "../bindings/ExchangeDetail";
import type { FuzzAttackMode } from "../bindings/FuzzAttackMode";
import type { FuzzBuiltinList } from "../bindings/FuzzBuiltinList";
import type { FuzzConfig } from "../bindings/FuzzConfig";
import type { FuzzPayloadSet } from "../bindings/FuzzPayloadSet";
import type { FuzzPreview } from "../bindings/FuzzPreview";
import type { FuzzResultQuery } from "../bindings/FuzzResultQuery";
import type { FuzzResultRow } from "../bindings/FuzzResultRow";
import type { FuzzResultsPage } from "../bindings/FuzzResultsPage";
import type { Highlight } from "../bindings/Highlight";
import type { TaskRow } from "../bindings/TaskRow";
import { formatTarget, parseTarget } from "../lib/repeaterTarget";
import { api, dialogs, toIpcError } from "../services/ipc";
import { useAppStore } from "./app";

export const MARKER = "§";

/** Rows of results loaded at once. */
const PAGE = 2000;
/** Delay before asking the engine for a preview of the edited attack. */
const PREVIEW_DELAY_MS = 250;

/** Payload sets an attack mode needs for `positions` positions. */
export function setsNeeded(mode: FuzzAttackMode, positions: number): number {
  return mode === "sniper" || mode === "batteringRam" ? 1 : Math.max(positions, 1);
}

/** Positions marked in a request text (pairs of §). */
export function countPositions(text: string): number {
  return Math.floor((text.split(MARKER).length - 1) / 2);
}

/** Wraps `text[start..end]` in markers (an empty selection adds an empty position). */
export function addMark(text: string, start: number, end: number): string {
  const [a, b] = start <= end ? [start, end] : [end, start];
  return `${text.slice(0, a)}${MARKER}${text.slice(a, b)}${MARKER}${text.slice(b)}`;
}

/** Status codes typed as "429, 503" (anything else is dropped). */
export function parseStatusList(text: string): number[] {
  const codes = text
    .split(/[\s,;]+/)
    .map((t) => Number(t))
    .filter((n) => Number.isInteger(n) && n >= 100 && n <= 599);
  return [...new Set(codes)].slice(0, 20);
}

/** Results columns that can be hidden. */
export const OPTIONAL_COLUMNS = [
  { id: "payload", label: "Payload" },
  { id: "status", label: "Status" },
  { id: "length", label: "Length" },
  { id: "duration", label: "Time" },
  { id: "error", label: "Error" },
  { id: "matches", label: "Match columns" },
  { id: "extracts", label: "Extract columns" },
] as const;
export type ResultColumn = (typeof OPTIONAL_COLUMNS)[number]["id"];

const COLUMNS_KEY = "chosproxy.fuzzer.hiddenColumns";

function loadHiddenColumns(): ResultColumn[] {
  try {
    const raw = JSON.parse(localStorage.getItem(COLUMNS_KEY) ?? "[]") as unknown;
    const known = new Set<string>(OPTIONAL_COLUMNS.map((c) => c.id));
    return Array.isArray(raw)
      ? raw.filter((c): c is ResultColumn => typeof c === "string" && known.has(c))
      : [];
  } catch {
    return [];
  }
}

export function emptySet(): FuzzPayloadSet {
  return { source: { kind: "list", items: [] }, processing: [] };
}

interface FuzzerState {
  config: FuzzConfig | null;
  targetText: string;
  lists: FuzzBuiltinList[];
  preview: FuzzPreview | null;
  tab: "setup" | "results";
  starting: boolean;
  attacks: TaskRow[];
  attackId: number | null;
  query: FuzzResultQuery;
  results: FuzzResultsPage | null;
  resultsError: string | null;
  selected: FuzzResultRow | null;
  detail: ExchangeDetail | null;
  /** Results columns hidden by the tester (remembered on this computer). */
  hiddenColumns: ResultColumn[];

  init(): Promise<void>;
  edit(patch: Partial<FuzzConfig>): void;
  setTargetText(text: string): void;
  setSet(index: number, set: FuzzPayloadSet): void;
  /** Marks the insertion points of `kinds` (none: parameters, cookies and body values). */
  autoMark(kinds?: PointKind[]): Promise<void>;
  setTab(tab: "setup" | "results"): void;
  /** Starts the attack (through `pool`, "" for the tool's own); resolves
   * to an error message or null. */
  start(pool?: string): Promise<string | null>;
  loadAttacks(): Promise<void>;
  selectAttack(id: number | null): void;
  loadResults(): Promise<void>;
  setQuery(patch: Partial<FuzzResultQuery>): void;
  selectResult(row: FuzzResultRow | null): void;
  toggleColumn(column: ResultColumn): void;
  /** Sets or clears a result's highlight colour. */
  setColor(id: number, color: Highlight | null): Promise<void>;
  /** Saves every result of the selected attack as CSV. */
  exportResults(): Promise<void>;
  /** "Send to Fuzzer" for a request text and its URL's target. */
  openRequest(target: { scheme: string; host: string; port: number }, text: string): Promise<void>;
  /** "Send to Fuzzer" for a stored request. */
  openExchange(exchangeId: number): Promise<void>;
  reset(): void;
}

let previewTimer: ReturnType<typeof setTimeout> | undefined;
let previewSeq = 0;
let resultsSeq = 0;

const INITIAL_QUERY: FuzzResultQuery = {
  sort: "index",
  descending: false,
  matchedOnly: false,
  differentStatusOnly: false,
  offset: 0,
  limit: PAGE,
};

export const useFuzzerStore = create<FuzzerState>((set, get) => {
  const notifyError = (prefix: string, e: unknown) =>
    useAppStore.getState().notify("error", `${prefix}: ${toIpcError(e).message}`);

  /** Keeps one payload set per needed position and asks for a preview. */
  const changed = (config: FuzzConfig) => {
    const needed = setsNeeded(config.mode, countPositions(config.template));
    let sets = config.payloadSets;
    if (sets.length < needed) {
      sets = [...sets, ...Array.from({ length: needed - sets.length }, emptySet)];
    }
    const next = sets === config.payloadSets ? config : { ...config, payloadSets: sets };
    set({ config: next });
    clearTimeout(previewTimer);
    previewTimer = setTimeout(() => {
      const mine = ++previewSeq;
      api
        .fuzzPreview(next)
        .then((preview) => {
          if (mine === previewSeq) set({ preview });
        })
        .catch(() => undefined);
    }, PREVIEW_DELAY_MS);
  };

  return {
    config: null,
    targetText: "",
    lists: [],
    preview: null,
    tab: "setup",
    starting: false,
    attacks: [],
    attackId: null,
    query: INITIAL_QUERY,
    results: null,
    resultsError: null,
    selected: null,
    detail: null,

    async init() {
      try {
        if (get().lists.length === 0) set({ lists: await api.fuzzBuiltinLists() });
        if (!get().config) {
          const config = await api.fuzzDefaults();
          set({ targetText: formatTarget(config.target) });
          changed(config);
        }
        await get().loadAttacks();
      } catch (e) {
        notifyError("Could not open the fuzzer", e);
      }
    },

    edit(patch) {
      const config = get().config;
      if (config) changed({ ...config, ...patch });
    },

    setTargetText(text) {
      set({ targetText: text });
      const target = parseTarget(text);
      if (!("error" in target)) get().edit({ target });
    },

    setSet(index, payloadSet) {
      const config = get().config;
      if (!config) return;
      const sets = [...config.payloadSets];
      sets[index] = payloadSet;
      changed({ ...config, payloadSets: sets });
    },

    async autoMark(kinds = []) {
      const config = get().config;
      if (!config) return;
      try {
        get().edit({ template: await api.fuzzAutoMark(config.template, kinds) });
      } catch (e) {
        notifyError("Could not mark the inputs", e);
      }
    },

    setTab: (tab) => set({ tab }),

    async start(pool = "") {
      const config = get().config;
      if (!config) return null;
      const target = parseTarget(get().targetText);
      if ("error" in target) return target.error;
      const needed = setsNeeded(config.mode, countPositions(config.template));
      set({ starting: true });
      try {
        const task = await api.fuzzStart(
          {
            ...config,
            target,
            payloadSets: config.payloadSets.slice(0, needed),
          },
          pool || null,
        );
        set({
          tab: "results",
          attackId: task.id,
          query: INITIAL_QUERY,
          selected: null,
          detail: null,
        });
        useAppStore.getState().notify("info", `${task.name} started`);
        await get().loadAttacks();
        await get().loadResults();
        return null;
      } catch (e) {
        return toIpcError(e).message;
      } finally {
        set({ starting: false });
      }
    },

    async loadAttacks() {
      try {
        const attacks = (await api.tasks()).filter((t) => t.kind === "fuzz");
        const attackId = get().attackId ?? attacks[0]?.id ?? null;
        set({ attacks, attackId });
      } catch (e) {
        if (toIpcError(e).kind !== "noProject") notifyError("Could not load attacks", e);
      }
    },

    selectAttack(id) {
      set({ attackId: id, results: null, selected: null, detail: null, query: INITIAL_QUERY });
      void get().loadResults();
    },

    async loadResults() {
      const id = get().attackId;
      if (id === null) {
        set({ results: null });
        return;
      }
      const mine = ++resultsSeq;
      try {
        const results = await api.fuzzResults(id, get().query);
        if (mine === resultsSeq) set({ results, resultsError: null });
      } catch (e) {
        if (mine === resultsSeq) set({ resultsError: toIpcError(e).message });
      }
    },

    setQuery(patch) {
      set((s) => ({ query: { ...s.query, ...patch, offset: 0 } }));
      void get().loadResults();
    },

    selectResult(row) {
      set({ selected: row, detail: null });
      if (row?.exchangeId == null) return;
      const id = row.exchangeId;
      api
        .exchangeDetail(id)
        .then((detail) => {
          if (get().selected?.exchangeId === id) set({ detail });
        })
        .catch((e: unknown) => notifyError("Could not load the request", e));
    },

    hiddenColumns: loadHiddenColumns(),

    toggleColumn(column) {
      const hidden = get().hiddenColumns.includes(column)
        ? get().hiddenColumns.filter((c) => c !== column)
        : [...get().hiddenColumns, column];
      set({ hiddenColumns: hidden });
      try {
        localStorage.setItem(COLUMNS_KEY, JSON.stringify(hidden));
      } catch {
        // Not remembered; the choice still applies now.
      }
    },

    async setColor(id, color) {
      try {
        const row = await api.fuzzSetColor(id, color);
        set((s) => ({
          results: s.results && {
            ...s.results,
            rows: s.results.rows.map((r) => (r.id === id ? row : r)),
          },
          selected: s.selected?.id === id ? row : s.selected,
        }));
      } catch (e) {
        notifyError("Could not change the highlight", e);
      }
    },

    async exportResults() {
      const id = get().attackId;
      if (id === null) return;
      const path = await dialogs.chooseCsvDestination(`fuzz-${id}.csv`);
      if (!path) return;
      try {
        const saved = await api.fuzzExport(id, path);
        useAppStore.getState().notify("info", `Results exported to ${saved}`);
      } catch (e) {
        notifyError("Export failed", e);
      }
    },

    async openRequest(target, text) {
      await get().init();
      set({ targetText: formatTarget(target), tab: "setup" });
      get().edit({ target, template: text.replaceAll(MARKER, "") });
      useAppStore.getState().setView("fuzzer");
    },

    async openExchange(exchangeId) {
      try {
        const snapshot = await api.repeaterSnapshot(exchangeId);
        if (snapshot.requestText === null) {
          useAppStore
            .getState()
            .notify("error", "That request cannot be edited as text (binary body)");
          return;
        }
        const row = snapshot.detail.row;
        await get().openRequest(
          { scheme: row.scheme, host: row.host, port: row.port },
          snapshot.requestText,
        );
      } catch (e) {
        notifyError("Could not send to the fuzzer", e);
      }
    },

    reset() {
      clearTimeout(previewTimer);
      set({
        attacks: [],
        attackId: null,
        results: null,
        resultsError: null,
        selected: null,
        detail: null,
        query: INITIAL_QUERY,
      });
    },
  };
});
