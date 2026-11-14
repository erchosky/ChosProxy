import { create } from "zustand";
import type { EngineEvent } from "../bindings/EngineEvent";
import type { LogEntry } from "../bindings/LogEntry";
import { throttle } from "../lib/throttle";
import { api, toIpcError } from "../services/ipc";

/** Events that add log entries (others are not recorded). */
const LOGGED: ReadonlySet<EngineEvent["type"]> = new Set([
  "listenerChanged",
  "projectChanged",
  "scopeChanged",
  "caChanged",
  "notice",
]);
const RELOAD_DELAY_MS = 150;

interface EventLogState {
  entries: LogEntry[];
  loaded: boolean;
  error: string | null;
  /** The view is open; only then are new events fetched. */
  active: boolean;

  load(): Promise<void>;
  clear(): Promise<void>;
  setActive(active: boolean): void;
  onEvent(event: EngineEvent): void;
}

export const useEventLogStore = create<EventLogState>((set, get) => {
  const reload = throttle(() => void get().load(), RELOAD_DELAY_MS);
  return {
    entries: [],
    loaded: false,
    error: null,
    active: false,

    async load() {
      try {
        set({ entries: await api.eventLog(), loaded: true, error: null });
      } catch (e) {
        set({ error: toIpcError(e).message, loaded: true });
      }
    },

    async clear() {
      try {
        await api.clearEventLog();
        set({ entries: [] });
      } catch (e) {
        set({ error: toIpcError(e).message });
      }
    },

    setActive(active) {
      set({ active });
      if (active) void get().load();
    },

    onEvent(event) {
      if (!get().active || !LOGGED.has(event.type)) return;
      // The engine records events on its own thread: read a little later, at
      // most once per interval.
      window.setTimeout(reload, RELOAD_DELAY_MS);
    },
  };
});
