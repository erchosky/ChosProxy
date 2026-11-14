// ChosOAST: configuration, payloads of the open project and the
// interactions received for them.
import { create } from "zustand";
import type { OastConfig } from "../bindings/OastConfig";
import type { OastInteractionDetail } from "../bindings/OastInteractionDetail";
import type { OastInteractionRow } from "../bindings/OastInteractionRow";
import type { OastPayloadRow } from "../bindings/OastPayloadRow";
import type { OastStatus } from "../bindings/OastStatus";
import { api, toIpcError } from "../services/ipc";
import { copyText } from "../services/clipboard";
import { useAppStore } from "./app";

type Detail =
  | { status: "idle" }
  | { status: "loading"; id: number }
  | { status: "ready"; id: number; detail: OastInteractionDetail }
  | { status: "error"; id: number; message: string };

interface OastState {
  config: OastConfig | null;
  status: OastStatus | null;
  payloads: OastPayloadRow[];
  interactions: OastInteractionRow[];
  /** Show only interactions of this identifier. */
  filterId: string | null;
  detail: Detail;
  loaded: boolean;
  error: string | null;
  saving: boolean;

  load(): Promise<void>;
  saveConfig(config: OastConfig): Promise<boolean>;
  testServer(config: OastConfig): Promise<void>;
  /** Creates a payload and copies `what` of it to the clipboard. */
  newPayload(note: string, copy?: "url" | "dns"): Promise<OastPayloadRow | null>;
  setFilter(id: string | null): Promise<void>;
  selectInteraction(id: number | null): void;
}

let detailSeq = 0;

let loadSeq = 0;

export const useOastStore = create<OastState>((set, get) => {
  const notify = (level: "info" | "error", message: string) =>
    useAppStore.getState().notify(level, message);

  return {
    config: null,
    status: null,
    payloads: [],
    interactions: [],
    filterId: null,
    detail: { status: "idle" },
    loaded: false,
    error: null,
    saving: false,

    async load() {
      const mine = ++loadSeq;
      const filterId = get().filterId;
      try {
        const hasProject = useAppStore.getState().project !== null;
        const [config, status, payloads, interactions] = await Promise.all([
          api.oastConfig(),
          api.oastStatus(),
          hasProject ? api.oastPayloads() : Promise.resolve([]),
          hasProject ? api.oastInteractions(filterId) : Promise.resolve([]),
        ]);
        // An older load, or one for another payload filter, is dropped.
        if (mine !== loadSeq || get().filterId !== filterId) return;
        set({ config, status, payloads, interactions, loaded: true, error: null });
      } catch (e) {
        if (mine === loadSeq) set({ loaded: true, error: toIpcError(e).message });
      }
    },

    async saveConfig(config) {
      set({ saving: true });
      try {
        const status = await api.oastSaveConfig(config);
        set({ config, status, saving: false });
        notify("info", status.error ? `OAST: ${status.error}` : "OAST settings saved");
        return status.error === null;
      } catch (e) {
        set({ saving: false });
        notify("error", toIpcError(e).message);
        return false;
      }
    },

    async testServer(config) {
      try {
        notify("info", await api.oastTestServer(config.selfHosted));
      } catch (e) {
        notify("error", toIpcError(e).message);
      }
    },

    async newPayload(note, copy) {
      try {
        const payload = await api.oastNewPayload(note);
        set((s) => ({ payloads: [payload, ...s.payloads] }));
        if (copy) {
          const text = copy === "dns" ? (payload.dnsName ?? payload.url) : payload.url;
          const ok = await copyText(text);
          notify(ok ? "info" : "error", ok ? `Copied ${text}` : "Could not copy to the clipboard");
        }
        return payload;
      } catch (e) {
        notify("error", toIpcError(e).message);
        return null;
      }
    },

    async setFilter(filterId) {
      set({ filterId, detail: { status: "idle" } });
      try {
        set({ interactions: await api.oastInteractions(filterId) });
      } catch (e) {
        set({ error: toIpcError(e).message });
      }
    },

    selectInteraction(id) {
      if (id === null) {
        set({ detail: { status: "idle" } });
        return;
      }
      const mine = ++detailSeq;
      set({ detail: { status: "loading", id } });
      api
        .oastInteraction(id)
        .then((detail) => {
          if (mine === detailSeq) set({ detail: { status: "ready", id, detail } });
        })
        .catch((e: unknown) => {
          if (mine === detailSeq)
            set({ detail: { status: "error", id, message: toIpcError(e).message } });
        });
    },
  };
});
