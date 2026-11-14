import { create } from "zustand";
import type { DecodeStep } from "../bindings/DecodeStep";
import type { Operation } from "../bindings/Operation";
import { api, toIpcError } from "../services/ipc";

const RUN_DELAY_MS = 200;

/** UTF-8 text → Base64 (the engine works on bytes). */
export function textToBase64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}

interface DecoderState {
  input: string;
  ops: Operation[];
  steps: DecodeStep[];
  error: string | null;
  running: boolean;
  /** Message after a smart decode (e.g. "nothing recognised"). */
  hint: string | null;

  setInput(input: string): void;
  setOps(ops: Operation[]): void;
  run(): Promise<void>;
  smart(): Promise<void>;
  /** Starts over with `input` (e.g. "Send to Decoder") and proposes a chain. */
  load(input: string): void;
}

let timer: number | undefined;
let seq = 0;

export const useDecoderStore = create<DecoderState>((set, get) => {
  function schedule() {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => void get().run(), RUN_DELAY_MS);
  }
  return {
    input: "",
    ops: [],
    steps: [],
    error: null,
    running: false,
    hint: null,

    setInput(input) {
      set({ input, hint: null });
      schedule();
    },

    setOps(ops) {
      set({ ops, hint: null });
      schedule();
    },

    async run() {
      const mine = ++seq;
      const { input, ops } = get();
      if (ops.length === 0) {
        set({ steps: [], error: null });
        return;
      }
      set({ running: true });
      try {
        const steps = await api.decode(textToBase64(input), ops);
        if (mine === seq) set({ steps, error: null });
      } catch (e) {
        if (mine === seq) set({ error: toIpcError(e).message, steps: [] });
      } finally {
        if (mine === seq) set({ running: false });
      }
    },

    async smart() {
      const mine = ++seq;
      try {
        const result = await api.smartDecode(textToBase64(get().input));
        if (mine !== seq) return;
        set({
          ops: result.ops,
          steps: result.steps,
          error: null,
          hint: result.ops.length === 0 ? "No known encoding recognised." : null,
        });
      } catch (e) {
        if (mine === seq) set({ error: toIpcError(e).message });
      }
    },

    load(input) {
      set({ input, ops: [], steps: [], hint: null, error: null });
      void get().smart();
    },
  };
});
