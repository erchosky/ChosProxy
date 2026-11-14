import { create } from "zustand";
import type { OrganizerDetail } from "../bindings/OrganizerDetail";
import type { OrganizerItem } from "../bindings/OrganizerItem";
import type { OrganizerUpdate } from "../bindings/OrganizerUpdate";
import { api, toIpcError } from "../services/ipc";

type Detail =
  | { status: "idle" }
  | { status: "loading"; id: number }
  | { status: "ready"; id: number; detail: OrganizerDetail }
  | { status: "error"; id: number; message: string };

interface OrganizerState {
  items: OrganizerItem[];
  loaded: boolean;
  error: string | null;
  selectedId: number | null;
  detail: Detail;

  load(): Promise<void>;
  select(id: number | null): void;
  /** Saves; resolves to an error message, or null on success. */
  update(id: number, update: OrganizerUpdate): Promise<string | null>;
  remove(id: number): Promise<string | null>;
  /** Moves an item to a collection ("" = none), keeping its other fields. */
  moveTo(id: number, collection: string): Promise<string | null>;
  reset(): void;
}

let loadSeq = 0;

export const useOrganizerStore = create<OrganizerState>((set, get) => ({
  items: [],
  loaded: false,
  error: null,
  selectedId: null,
  detail: { status: "idle" },

  async load() {
    // Loads overlap (an edit, a drop and an engine event close together):
    // only the newest answer is kept.
    const mine = ++loadSeq;
    try {
      const items = await api.organizerList();
      if (mine === loadSeq) set({ items, loaded: true, error: null });
    } catch (e) {
      const err = toIpcError(e);
      if (mine === loadSeq) {
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
    set({ detail: { status: "loading", id } });
    api
      .organizerDetail(id)
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
      await api.organizerUpdate(id, update);
      await get().load();
      get().select(id);
      return null;
    } catch (e) {
      return toIpcError(e).message;
    }
  },

  async moveTo(id, collection) {
    const item = get().items.find((i) => i.id === id);
    if (!item) return "the item is gone";
    if (item.collection === collection) return null;
    try {
      await api.organizerUpdate(id, {
        name: item.name,
        notes: item.notes,
        tags: item.tags,
        status: item.status,
        severity: item.severity,
        collection,
      });
      await get().load();
      // An open editor shows the new collection.
      if (get().selectedId === id) get().select(id);
      return null;
    } catch (e) {
      return toIpcError(e).message;
    }
  },

  async remove(id) {
    try {
      await api.organizerDelete(id);
      if (get().selectedId === id) get().select(null);
      await get().load();
      return null;
    } catch (e) {
      return toIpcError(e).message;
    }
  },

  reset: () =>
    set({ items: [], loaded: false, error: null, selectedId: null, detail: { status: "idle" } }),
}));
