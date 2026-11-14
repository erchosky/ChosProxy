// DOM Inspector: the instrumented lab browser session and what it observed.
import { create } from "zustand";
import type { DomEventRow } from "../bindings/DomEventRow";
import type { DomStatus } from "../bindings/DomStatus";
import { api, toIpcError } from "../services/ipc";
import { useAppStore } from "./app";

interface DomState {
  status: DomStatus | null;
  events: DomEventRow[];
  selectedId: number | null;
  /** Kinds shown (empty = all). */
  kinds: string[];
  loaded: boolean;
  error: string | null;
  launching: boolean;

  load(): Promise<void>;
  launch(browserId: string | null, url: string): Promise<void>;
  clear(): Promise<void>;
  navigate(url: string): Promise<void>;
  select(id: number | null): void;
  toggleKind(kind: string): void;
}

export const useDomStore = create<DomState>((set, get) => ({
  status: null,
  events: [],
  selectedId: null,
  kinds: [],
  loaded: false,
  error: null,
  launching: false,

  async load() {
    try {
      const hasProject = useAppStore.getState().project !== null;
      const [status, events] = await Promise.all([
        api.domStatus(),
        hasProject ? api.domEvents() : Promise.resolve([]),
      ]);
      set({ status, events, loaded: true, error: null });
    } catch (e) {
      set({ loaded: true, error: toIpcError(e).message });
    }
  },

  async launch(browserId, url) {
    set({ launching: true });
    try {
      const browser = await api.domLaunchBrowser(browserId, url.trim() === "" ? null : url.trim());
      useAppStore.setState({ browser });
      useAppStore.getState().notify("info", "Lab browser opened with the DOM Inspector");
    } catch (e) {
      useAppStore.getState().notify("error", toIpcError(e).message);
    } finally {
      set({ launching: false });
      void get().load();
    }
  },

  async navigate(url) {
    try {
      await api.domNavigate(url);
    } catch (e) {
      useAppStore.getState().notify("error", toIpcError(e).message);
    }
  },

  async clear() {
    try {
      await api.domClear();
      set({ events: [], selectedId: null });
    } catch (e) {
      useAppStore.getState().notify("error", toIpcError(e).message);
    }
  },

  select(selectedId) {
    set({ selectedId });
  },

  toggleKind(kind) {
    set((s) => ({
      kinds: s.kinds.includes(kind) ? s.kinds.filter((k) => k !== kind) : [...s.kinds, kind],
    }));
  },
}));

/** Splits `text` around the first occurrence of `part` for highlighting. */
export function splitMatch(
  text: string,
  part: string | null,
): { before: string; match: string; after: string } | null {
  if (!part) return null;
  const at = text.indexOf(part);
  if (at < 0) return null;
  return {
    before: text.slice(0, at),
    match: text.slice(at, at + part.length),
    after: text.slice(at + part.length),
  };
}
