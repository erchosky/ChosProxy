import { create } from "zustand";
import type { HeldDetail } from "../bindings/HeldDetail";
import type { InterceptState } from "../bindings/InterceptState";
import { api, toIpcError } from "../services/ipc";
import { useAppStore } from "./app";

interface InterceptStore {
  state: InterceptState;
  selectedId: number | null;
  detail: HeldDetail | null;
  /** Edited text of the selected message (null = unchanged). */
  draft: string | null;
  updateContentLength: boolean;
  loading: boolean;

  refresh(): Promise<void>;
  setEnabled(enabled: boolean): Promise<void>;
  select(id: number | null): Promise<void>;
  setDraft(text: string): void;
  setUpdateContentLength(on: boolean): void;
  forward(interceptResponse?: boolean): Promise<void>;
  drop(): Promise<void>;
  forwardAll(): Promise<void>;
  unpack(): Promise<void>;
}

function report(action: string, e: unknown) {
  useAppStore.getState().notify("error", `${action} failed: ${toIpcError(e).message}`);
}

export const useInterceptStore = create<InterceptStore>((set, get) => {
  /** Selects the next message after the current one was decided. */
  async function afterDecision() {
    set({ selectedId: null, detail: null, draft: null });
    await get().refresh();
  }

  return {
    state: { enabled: false, pending: [] },
    selectedId: null,
    detail: null,
    draft: null,
    updateContentLength: true,
    loading: false,

    async refresh() {
      try {
        const state = await api.interceptState();
        set({ state });
        const { selectedId } = get();
        const stillThere = state.pending.some((p) => p.id === selectedId);
        if (!stillThere) {
          const first = state.pending[0]?.id ?? null;
          await get().select(first);
        }
      } catch (e) {
        report("Reading the Intercept queue", e);
      }
    },

    async setEnabled(enabled) {
      try {
        set({ state: await api.setIntercept(enabled) });
        await get().refresh();
      } catch (e) {
        report("Switching Intercept", e);
      }
    },

    async select(id) {
      if (id === null) {
        set({ selectedId: null, detail: null, draft: null });
        return;
      }
      if (id === get().selectedId && get().detail) return;
      set({ selectedId: id, loading: true, draft: null });
      try {
        const detail = await api.heldMessage(id);
        if (get().selectedId === id) set({ detail });
      } catch (e) {
        // Decided elsewhere or connection closed: refresh the list.
        if (get().selectedId === id) set({ selectedId: null, detail: null });
        void get().refresh();
        report("Opening the paused message", e);
      } finally {
        set({ loading: false });
      }
    },

    setDraft(text) {
      const original = get().detail?.text ?? null;
      set({ draft: text === original ? null : text });
    },

    setUpdateContentLength(on) {
      set({ updateContentLength: on });
    },

    async forward(interceptResponse = false) {
      const { selectedId, draft, detail, updateContentLength } = get();
      if (selectedId === null || !detail) return;
      try {
        const edit = draft === null ? null : { text: draft, updateContentLength };
        await api.forwardHeld(selectedId, edit, interceptResponse);
        await afterDecision();
      } catch (e) {
        report("Forward", e);
        void get().refresh();
      }
    },

    async drop() {
      const { selectedId } = get();
      if (selectedId === null) return;
      try {
        await api.dropHeld(selectedId);
        await afterDecision();
      } catch (e) {
        report("Drop", e);
        void get().refresh();
      }
    },

    async forwardAll() {
      try {
        await api.forwardAllHeld();
        await afterDecision();
      } catch (e) {
        report("Forward all", e);
      }
    },

    async unpack() {
      const { selectedId } = get();
      if (selectedId === null) return;
      try {
        const detail = await api.unpackHeld(selectedId);
        set({ detail, draft: null });
      } catch (e) {
        report("Unpack", e);
      }
    },
  };
});
