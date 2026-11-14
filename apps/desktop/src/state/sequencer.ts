// Sequencer: token captures (tasks of kind "sequencer") and their analysis,
// or the analysis of tokens pasted by the tester.
import { create } from "zustand";
import type { CaptureConfig } from "../bindings/CaptureConfig";
import type { TaskRow } from "../bindings/TaskRow";
import type { TokenAnalysis } from "../bindings/TokenAnalysis";
import type { TokenCandidate } from "../bindings/TokenCandidate";
import { formatTarget, parseTarget } from "../lib/repeaterTarget";
import { api, toIpcError } from "../services/ipc";
import { useAppStore } from "./app";

/** What the analysis panel shows. */
export type Source = { kind: "capture"; id: number } | { kind: "pasted" } | null;

interface SequencerState {
  config: CaptureConfig | null;
  targetText: string;
  candidates: TokenCandidate[];
  pasted: string;
  starting: boolean;
  captures: TaskRow[];
  source: Source;
  analysis: TokenAnalysis | null;
  analysisError: string | null;
  analyzing: boolean;
  /** First tokens of the analyzed sample. */
  sample: string[];

  init(): Promise<void>;
  edit(patch: Partial<CaptureConfig>): void;
  setTargetText(text: string): void;
  setPasted(text: string): void;
  /** "Send to Sequencer" for a stored request. */
  openExchange(exchangeId: number): Promise<void>;
  /** Starts a capture; resolves to an error message or null. */
  start(): Promise<string | null>;
  loadCaptures(): Promise<void>;
  selectCapture(id: number): Promise<void>;
  analyzePasted(): Promise<void>;
  reset(): void;
}

let analysisSeq = 0;

export const useSequencerStore = create<SequencerState>((set, get) => ({
  config: null,
  targetText: "",
  candidates: [],
  pasted: "",
  starting: false,
  captures: [],
  source: null,
  analysis: null,
  analysisError: null,
  analyzing: false,
  sample: [],

  async init() {
    try {
      if (!get().config) {
        const config = await api.sequencerDefaults();
        set({
          config,
          targetText: formatTarget({ scheme: config.scheme, host: config.host, port: config.port }),
        });
      }
      await get().loadCaptures();
    } catch (e) {
      useAppStore
        .getState()
        .notify("error", `Could not open the sequencer: ${toIpcError(e).message}`);
    }
  },

  edit(patch) {
    const config = get().config;
    if (config) set({ config: { ...config, ...patch } });
  },

  setTargetText(text) {
    set({ targetText: text });
    const t = parseTarget(text);
    if (!("error" in t)) get().edit({ scheme: t.scheme, host: t.host, port: t.port });
  },

  setPasted: (pasted) => set({ pasted }),

  async openExchange(exchangeId) {
    try {
      const prepared = await api.sequencerPrepare(exchangeId);
      const c = prepared.config;
      set({
        config: c,
        candidates: prepared.candidates,
        targetText: formatTarget({ scheme: c.scheme, host: c.host, port: c.port }),
      });
      useAppStore.getState().setView("sequencer");
      if (prepared.candidates.length === 0) {
        useAppStore
          .getState()
          .notify("info", "No token found in that response: choose where the token is");
      }
      await get().loadCaptures();
    } catch (e) {
      useAppStore
        .getState()
        .notify("error", `Could not send to the sequencer: ${toIpcError(e).message}`);
    }
  },

  async start() {
    const config = get().config;
    if (!config) return null;
    const t = parseTarget(get().targetText);
    if ("error" in t) return t.error;
    set({ starting: true });
    try {
      const task = await api.sequencerStart({
        ...config,
        scheme: t.scheme,
        host: t.host,
        port: t.port,
      });
      useAppStore.getState().notify("info", `${task.name} started`);
      await get().loadCaptures();
      await get().selectCapture(task.id);
      return null;
    } catch (e) {
      return toIpcError(e).message;
    } finally {
      set({ starting: false });
    }
  },

  async loadCaptures() {
    try {
      const captures = (await api.tasks()).filter((t) => t.kind === "sequencer");
      const source = get().source;
      const before =
        source?.kind === "capture" ? get().captures.find((c) => c.id === source.id) : undefined;
      set({ captures });
      // The capture on screen just finished: analyze all its tokens.
      const now = source?.kind === "capture" ? captures.find((c) => c.id === source.id) : undefined;
      const done = (s?: string) => s === "completed" || s === "cancelled" || s === "failed";
      if (source?.kind === "capture" && now && done(now.status) && !done(before?.status)) {
        await get().selectCapture(source.id);
      }
    } catch (e) {
      if (toIpcError(e).kind !== "noProject") {
        useAppStore.getState().notify("error", `Could not load captures: ${toIpcError(e).message}`);
      }
    }
  },

  async selectCapture(id) {
    const mine = ++analysisSeq;
    set({ source: { kind: "capture", id }, analyzing: true, analysisError: null });
    try {
      const tokens = await api.sequencerTokens(id);
      const analysis = tokens.length >= 2 ? await api.sequencerAnalysis(id) : null;
      if (mine !== analysisSeq) return;
      set({
        analysis,
        sample: tokens.slice(0, 20),
        analysisError: analysis ? null : "Not enough tokens yet (at least two are needed).",
      });
    } catch (e) {
      if (mine === analysisSeq) set({ analysis: null, analysisError: toIpcError(e).message });
    } finally {
      if (mine === analysisSeq) set({ analyzing: false });
    }
  },

  async analyzePasted() {
    const tokens = get()
      .pasted.split(/\r?\n/)
      .map((t) => t.trim())
      .filter(Boolean);
    const mine = ++analysisSeq;
    set({ source: { kind: "pasted" }, analyzing: true, analysisError: null });
    try {
      const analysis = await api.sequencerAnalyze(tokens);
      if (mine === analysisSeq) set({ analysis, sample: tokens.slice(0, 20) });
    } catch (e) {
      if (mine === analysisSeq) set({ analysis: null, analysisError: toIpcError(e).message });
    } finally {
      if (mine === analysisSeq) set({ analyzing: false });
    }
  },

  reset: () =>
    set({
      captures: [],
      source: null,
      analysis: null,
      analysisError: null,
      sample: [],
      candidates: [],
    }),
}));
