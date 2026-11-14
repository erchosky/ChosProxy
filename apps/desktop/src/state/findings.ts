import { create } from "zustand";
import type { FindingDetail } from "../bindings/FindingDetail";
import type { FindingRow } from "../bindings/FindingRow";
import type { FindingSeverity } from "../bindings/FindingSeverity";
import type { FindingStatus } from "../bindings/FindingStatus";
import type { FindingUpdate } from "../bindings/FindingUpdate";
import type { PassiveStatus } from "../bindings/PassiveStatus";
import { api, toIpcError } from "../services/ipc";
import { useAppStore } from "./app";

export interface FindingFilter {
  search: string;
  severities: FindingSeverity[];
  statuses: FindingStatus[];
  inScopeOnly: boolean;
  /** "" for every source. */
  source: "" | "manual" | "passive" | "plugin" | "oast" | "dom";
}

/** False positives and accepted risks are hidden until asked for. */
export const DEFAULT_FINDING_FILTER: FindingFilter = {
  search: "",
  severities: [],
  statuses: ["open", "confirmed", "fixed"],
  inScopeOnly: false,
  source: "",
};

type Detail =
  | { status: "idle" }
  | { status: "loading"; id: number }
  | { status: "ready"; id: number; detail: FindingDetail }
  | { status: "error"; id: number; message: string };

interface FindingsState {
  rows: FindingRow[];
  loaded: boolean;
  error: string | null;
  filter: FindingFilter;
  selectedId: number | null;
  detail: Detail;
  /** Exchange waiting in the "Add to finding" dialog. */
  adding: { exchangeId: number; label: string } | null;
  /** Where passive analysis stands (null until loaded). */
  passive: PassiveStatus | null;

  load(): Promise<void>;
  loadPassive(): Promise<void>;
  /** Analyzes all traffic of the project again. */
  reanalyze(): Promise<void>;
  setFilter(patch: Partial<FindingFilter>): void;
  select(id: number | null): void;
  update(id: number, update: FindingUpdate): Promise<string | null>;
  remove(id: number): Promise<string | null>;
  removeEvidence(evidenceId: number): Promise<void>;
  openAdd(exchangeId: number, label: string): void;
  closeAdd(): void;
  reset(): void;
}

let seq = 0;

export const useFindingsStore = create<FindingsState>((set, get) => ({
  rows: [],
  loaded: false,
  error: null,
  filter: DEFAULT_FINDING_FILTER,
  selectedId: null,
  detail: { status: "idle" },
  adding: null,
  passive: null,

  async load() {
    const mine = ++seq;
    const { filter } = get();
    try {
      const rows = await api.findings({
        search: filter.search.trim() === "" ? null : filter.search.trim(),
        severities: filter.severities,
        statuses: filter.statuses,
        inScopeOnly: filter.inScopeOnly,
        source: filter.source === "" ? null : filter.source,
      });
      if (mine === seq) set({ rows, loaded: true, error: null });
    } catch (e) {
      const err = toIpcError(e);
      if (mine === seq) set({ loaded: true, error: err.kind === "noProject" ? null : err.message });
    }
  },

  async loadPassive() {
    try {
      set({ passive: await api.passiveStatus() });
    } catch {
      set({ passive: null });
    }
  },

  async reanalyze() {
    try {
      set({ passive: await api.passiveReanalyze() });
      useAppStore.getState().notify("info", "Analyzing all traffic of the project again");
    } catch (e) {
      useAppStore
        .getState()
        .notify("error", `Could not start the analysis: ${toIpcError(e).message}`);
    }
  },

  setFilter(patch) {
    set((s) => ({ filter: { ...s.filter, ...patch } }));
    void get().load();
  },

  select(id) {
    set({ selectedId: id });
    if (id === null) {
      set({ detail: { status: "idle" } });
      return;
    }
    set({ detail: { status: "loading", id } });
    api
      .findingDetail(id)
      .then((detail) => {
        if (get().selectedId === id) set({ detail: { status: "ready", id, detail } });
      })
      .catch((e: unknown) => {
        if (get().selectedId === id) {
          set({ detail: { status: "error", id, message: toIpcError(e).message } });
        }
      });
  },

  async update(id, update) {
    try {
      await api.findingUpdate(id, update);
      await get().load();
      get().select(id);
      void useAppStore.getState().refreshStats();
      return null;
    } catch (e) {
      return toIpcError(e).message;
    }
  },

  async remove(id) {
    try {
      await api.findingDelete(id);
      get().select(null);
      await get().load();
      void useAppStore.getState().refreshStats();
      return null;
    } catch (e) {
      return toIpcError(e).message;
    }
  },

  async removeEvidence(evidenceId) {
    const id = get().selectedId;
    try {
      await api.findingRemoveEvidence(evidenceId);
      await get().load();
      if (id !== null) get().select(id);
    } catch (e) {
      useAppStore
        .getState()
        .notify("error", `Could not remove the evidence: ${toIpcError(e).message}`);
    }
  },

  openAdd: (exchangeId, label) => set({ adding: { exchangeId, label } }),
  closeAdd: () => set({ adding: null }),

  reset: () =>
    set({
      rows: [],
      loaded: false,
      error: null,
      selectedId: null,
      detail: { status: "idle" },
      adding: null,
      passive: null,
    }),
}));
