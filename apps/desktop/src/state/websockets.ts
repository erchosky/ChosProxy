import { create } from "zustand";
import type { WsConnectionRow } from "../bindings/WsConnectionRow";
import type { WsDirection } from "../bindings/WsDirection";
import type { WsMessageDetail } from "../bindings/WsMessageDetail";
import type { WsMessagePage } from "../bindings/WsMessagePage";
import { throttle } from "../lib/throttle";
import { api, dialogs, toIpcError } from "../services/ipc";
import { useAppStore } from "./app";

/** Messages fetched per page. */
export const WS_PAGE_SIZE = 500;
const RELOAD_DELAY_MS = 400;

export interface WsMessageFilter {
  direction: WsDirection | null;
  hideControl: boolean;
  search: string;
  regex: boolean;
}

type Detail =
  | { status: "idle" }
  | { status: "loading"; id: number }
  | { status: "ready"; id: number; detail: WsMessageDetail }
  | { status: "error"; id: number; message: string };

interface WebSocketsState {
  connections: WsConnectionRow[];
  loaded: boolean;
  error: string | null;
  connectionSearch: string;
  inScopeOnly: boolean;
  selectedConnection: number | null;
  filter: WsMessageFilter;
  page: WsMessagePage | null;
  /** Index of the first loaded message. */
  offset: number;
  messagesError: string | null;
  selectedMessage: number | null;
  detail: Detail;
  active: boolean;

  load(): Promise<void>;
  setConnectionFilter(patch: { search?: string; inScopeOnly?: boolean }): void;
  selectConnection(id: number | null): void;
  setFilter(patch: Partial<WsMessageFilter>): void;
  loadMessages(offset?: number): Promise<void>;
  selectMessage(id: number | null): void;
  /** Shows one message of a connection, filtering on `search` so it is listed. */
  openMessage(connectionId: number, messageId: number, search: string): Promise<void>;
  exportConnection(): Promise<void>;
  setActive(active: boolean): void;
  onChanged(): void;
  reset(): void;
}

let messageSeq = 0;

export const useWebSocketsStore = create<WebSocketsState>((set, get) => {
  // A throttle, not a debounce: live sockets send events continuously.
  const refresh = throttle(() => {
    void get().load();
    void get().loadMessages();
  }, RELOAD_DELAY_MS);
  return {
    connections: [],
    loaded: false,
    error: null,
    connectionSearch: "",
    inScopeOnly: false,
    selectedConnection: null,
    filter: { direction: null, hideControl: true, search: "", regex: false },
    page: null,
    offset: 0,
    messagesError: null,
    selectedMessage: null,
    detail: { status: "idle" },
    active: false,

    async load() {
      const { connectionSearch, inScopeOnly } = get();
      try {
        const connections = await api.wsConnections({
          search: connectionSearch.trim() === "" ? null : connectionSearch.trim(),
          inScopeOnly,
        });
        set({ connections, loaded: true, error: null });
      } catch (e) {
        const err = toIpcError(e);
        set({ loaded: true, error: err.kind === "noProject" ? null : err.message });
      }
    },

    setConnectionFilter(patch) {
      set({
        connectionSearch: patch.search ?? get().connectionSearch,
        inScopeOnly: patch.inScopeOnly ?? get().inScopeOnly,
      });
      void get().load();
    },

    selectConnection(id) {
      set({
        selectedConnection: id,
        page: null,
        offset: 0,
        selectedMessage: null,
        detail: { status: "idle" },
      });
      if (id !== null) void get().loadMessages(0);
    },

    setFilter(patch) {
      set((s) => ({ filter: { ...s.filter, ...patch } }));
      void get().loadMessages(0);
    },

    async loadMessages(offset = get().offset) {
      const id = get().selectedConnection;
      if (id === null) return;
      const mine = ++messageSeq;
      const { filter } = get();
      try {
        const page = await api.wsMessages({
          connectionId: id,
          direction: filter.direction,
          hideControl: filter.hideControl,
          search: filter.search.trim() === "" ? null : filter.search.trim(),
          regex: filter.regex,
          offset,
          limit: WS_PAGE_SIZE,
        });
        if (mine === messageSeq) set({ page, offset, messagesError: null });
      } catch (e) {
        if (mine === messageSeq) set({ messagesError: toIpcError(e).message, page: null });
      }
    },

    async openMessage(connectionId, messageId, search) {
      set({
        selectedConnection: connectionId,
        filter: { direction: null, hideControl: false, search, regex: false },
        page: null,
        offset: 0,
      });
      await get().loadMessages(0);
      get().selectMessage(messageId);
    },

    selectMessage(id) {
      set({ selectedMessage: id });
      if (id === null) {
        set({ detail: { status: "idle" } });
        return;
      }
      set({ detail: { status: "loading", id } });
      api
        .wsMessage(id)
        .then((detail) => {
          if (get().selectedMessage === id) set({ detail: { status: "ready", id, detail } });
        })
        .catch((e: unknown) => {
          if (get().selectedMessage === id) {
            set({ detail: { status: "error", id, message: toIpcError(e).message } });
          }
        });
    },

    async exportConnection() {
      const id = get().selectedConnection;
      if (id === null) return;
      const path = await dialogs.chooseJsonDestination(`websocket-${id}.json`);
      if (!path) return;
      const { notify } = useAppStore.getState();
      try {
        notify("info", `WebSocket connection exported to ${await api.wsExport(id, path)}`);
      } catch (e) {
        notify("error", `Export failed: ${toIpcError(e).message}`);
      }
    },

    setActive(active) {
      set({ active });
      if (active) void get().load();
    },

    onChanged() {
      if (get().active) refresh();
    },

    reset() {
      messageSeq++;
      set({
        connections: [],
        loaded: false,
        error: null,
        selectedConnection: null,
        page: null,
        offset: 0,
        selectedMessage: null,
        detail: { status: "idle" },
      });
    },
  };
});
