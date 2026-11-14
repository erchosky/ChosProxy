import { create } from "zustand";
import type { FindingRow } from "../bindings/FindingRow";
import type { ReportFormat } from "../bindings/ReportFormat";
import type { ReportOptions } from "../bindings/ReportOptions";
import { api, dialogs, toIpcError } from "../services/ipc";
import { useAppStore } from "./app";

export const DEFAULT_METHODOLOGY =
  "Manual testing of the systems listed in the scope, with the owner's authorization. " +
  "HTTP and WebSocket traffic was captured and reviewed with ChosProxy; each finding was " +
  "verified by hand and documented with the captured requests and responses as evidence.";

/** Texts and options of the report (without the finding selection). */
export type ReportForm = Omit<ReportOptions, "findingIds">;

export const DEFAULT_FORM: ReportForm = {
  title: "Security assessment report",
  client: "",
  author: "",
  executiveSummary: "",
  methodology: DEFAULT_METHODOLOGY,
  includeEvidence: true,
  redactSecrets: true,
  maxEvidenceKib: 64,
};

/** Included unless chosen otherwise: everything but false positives. */
const includedByDefault = (r: FindingRow) => r.status !== "falsePositive";

/** Findings included: explicit choices first, the default for the rest. */
export const selection = (rows: FindingRow[], choices: Record<number, boolean>) =>
  rows.filter((r) => choices[r.id] ?? includedByDefault(r)).map((r) => r.id);

interface ReportState {
  form: ReportForm;
  findings: FindingRow[];
  /** Explicit include/exclude choices; other findings follow the default,
   * so findings documented later are included without a visit here. */
  choices: Record<number, boolean>;
  /** Included finding ids; null until the findings are first loaded. */
  selected: number[] | null;
  error: string | null;
  exporting: ReportFormat | null;

  load(): Promise<void>;
  patch(patch: Partial<ReportForm>): void;
  /** Includes exactly these findings. */
  setSelected(ids: number[]): void;
  /** Drops explicit choices: everything but false positives. */
  chooseDefault(): void;
  options(): ReportOptions;
  exportAs(format: ReportFormat): Promise<void>;
  reset(): void;
}

/** File name for a report title: letters and digits of any language kept. */
export const reportFileName = (title: string) =>
  title
    .trim()
    .replace(/[^\p{L}\p{N}_.-]+/gu, "-")
    .replace(/^[-.]+|[-.]+$/g, "") || "report";

/** Only the latest load may write: an answer for a closed project is dropped. */
let loadSeq = 0;

export const useReportStore = create<ReportState>()((set, get) => ({
  form: DEFAULT_FORM,
  findings: [],
  choices: {},
  selected: null,
  error: null,
  exporting: null,

  async load() {
    const mine = ++loadSeq;
    try {
      const findings = await api.findings({
        search: null,
        severities: [],
        statuses: [],
        inScopeOnly: false,
        source: null,
      });
      if (mine !== loadSeq) return;
      set({ findings, error: null, selected: selection(findings, get().choices) });
    } catch (e) {
      if (mine === loadSeq) set({ error: toIpcError(e).message });
    }
  },

  patch(patch) {
    set({ form: { ...get().form, ...patch } });
  },

  setSelected(ids) {
    const chosen = new Set(ids);
    const { findings } = get();
    const choices = Object.fromEntries(findings.map((f) => [f.id, chosen.has(f.id)]));
    set({ choices, selected: selection(findings, choices) });
  },

  chooseDefault() {
    set({ choices: {}, selected: selection(get().findings, {}) });
  },

  options() {
    const { form, selected } = get();
    return { ...form, findingIds: selected ?? [] };
  },

  async exportAs(format) {
    try {
      const path = await dialogs.chooseReportDestination(format, reportFileName(get().form.title));
      if (!path) return;
      set({ exporting: format, error: null });
      const saved = await api.exportReport(get().options(), format, path);
      useAppStore.getState().notify("info", `Report saved to ${saved}`);
    } catch (e) {
      set({ error: toIpcError(e).message });
    } finally {
      set({ exporting: null });
    }
  },

  // Texts stay for the next project; the selection belongs to this one.
  reset() {
    loadSeq++;
    set({ findings: [], choices: {}, selected: null, error: null, exporting: null });
  },
}));
