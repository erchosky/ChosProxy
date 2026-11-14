import { create } from "zustand";
import type { AgentApproval } from "../bindings/AgentApproval";
import type { AgentState } from "../bindings/AgentState";
import type { AiActivity } from "../bindings/AiActivity";
import type { TaskRow } from "../bindings/TaskRow";
import { api, toIpcError } from "../services/ipc";
import { useAppStore } from "./app";

/** One line of an agent transcript, from the AI activity log (M30). */
export interface TranscriptItem {
  id: number;
  at: number;
  /** goal, thinking (model text before tool calls), tool, message, answer, note. */
  kind: "goal" | "thinking" | "tool" | "message" | "answer" | "note";
  title: string;
  /** Markdown or plain text shown under the title. */
  body: string;
  /** For tools: allow / ask / deny and what happened. */
  permission?: string;
  decision?: string;
}

const DECISIONS: Record<string, string> = {
  ran: "ran",
  approved: "approved by you",
  rejected: "rejected by you",
  denied: "refused (not allowed)",
  failed: "refused or failed",
};

/** The transcript of an agent task, oldest first. */
export function transcript(rows: AiActivity[]): TranscriptItem[] {
  const out: TranscriptItem[] = [];
  for (const r of rows) {
    const base = { id: r.id, at: r.at };
    switch (r.action) {
      case "goal":
        out.push({ ...base, kind: "goal", title: "Goal", body: r.detail });
        break;
      case "model":
        // The model's text before its tool calls (the final answer has its
        // own row).
        if (r.result !== "answer" && r.detail.trim() !== "")
          out.push({ ...base, kind: "thinking", title: "ChosAgent", body: r.detail });
        break;
      case "tool":
        out.push({
          ...base,
          kind: "tool",
          title: `${r.tool}${r.target ? ` · ${r.target}` : ""}`,
          body: r.result,
          permission: r.permission,
          decision: DECISIONS[r.decision] ?? r.decision,
        });
        break;
      case "message":
        out.push({ ...base, kind: "message", title: "You", body: r.detail });
        break;
      case "answer":
        out.push({ ...base, kind: "answer", title: "Answer", body: r.detail });
        break;
      case "outgoing":
        out.push({
          ...base,
          kind: "note",
          title: `Context for the external model ${r.decision === "approved" ? "approved" : "rejected"}`,
          body: r.result,
        });
        break;
      case "resume":
        out.push({
          ...base,
          kind: "note",
          title: `Continues ${r.target || "an earlier task"}`,
          body: r.detail,
        });
        break;
      case "stop":
      case "error":
        out.push({
          ...base,
          kind: "note",
          title: r.action === "stop" ? "Stopped" : "Error",
          body: r.result,
        });
        break;
    }
  }
  return out;
}

/**
 * True for an agent task that ended without ending itself: cancelled, and
 * its log has no answer, stop or error at the end (the project was closed
 * while it ran, or the app quit).
 */
export function interrupted(task: Pick<TaskRow, "status">, rows: AiActivity[]): boolean {
  if (task.status !== "cancelled") return false;
  const last = rows.at(-1);
  return last !== undefined && !["answer", "stop", "error"].includes(last.action);
}

const EMPTY_STATE: AgentState = {
  running: false,
  plan: [],
  approvals: [],
  queued: [],
  steps: 0,
  sends: 0,
};

interface AgentStore {
  tasks: TaskRow[];
  selected: number | null;
  /** Activity of the selected task. */
  activity: AiActivity[];
  state: AgentState;
  /** Calls waiting for the tester, in every agent task. */
  approvals: AgentApproval[];
  /** History ids to attach to the next task (from a request menu). */
  draftIds: string;
  error: string | null;
  load(): Promise<void>;
  select(id: number | null): Promise<void>;
  refresh(): Promise<void>;
  setDraftIds(ids: string): void;
  /** Opens ChosAgent with a request attached. */
  startWith(exchangeId: number): void;
  /** Forgets everything (another project was opened). */
  reset(): void;
}

export const useAgentStore = create<AgentStore>((set, get) => ({
  tasks: [],
  selected: null,
  activity: [],
  state: EMPTY_STATE,
  approvals: [],
  draftIds: "",
  error: null,
  load: async () => {
    try {
      const [tasks, approvals] = await Promise.all([api.tasks(), api.agentApprovals()]);
      const agents = tasks.filter((t) => t.kind === "agent");
      set({ tasks: agents, approvals, error: null });
      const sel = get().selected;
      if (sel === null && agents[0]) await get().select(agents[0].id);
      else if (sel !== null) await get().refresh();
    } catch (e) {
      set({ error: toIpcError(e).message });
    }
  },
  select: async (id) => {
    set({ selected: id, activity: [], state: EMPTY_STATE });
    await get().refresh();
  },
  refresh: async () => {
    const id = get().selected;
    try {
      const approvals = await api.agentApprovals();
      set({ approvals });
      if (id === null) return;
      const known = get().activity;
      const after = known.at(-1)?.id ?? 0;
      const [more, state] = await Promise.all([api.aiActivity(id, after, 500), api.agentState(id)]);
      if (get().selected !== id) return;
      // Refreshes can overlap: keep only rows not appended yet.
      const last = get().activity.at(-1)?.id ?? 0;
      const fresh = more.filter((r) => r.id > last);
      set({ activity: fresh.length ? [...get().activity, ...fresh] : get().activity, state });
    } catch (e) {
      set({ error: toIpcError(e).message });
    }
  },
  setDraftIds: (draftIds) => set({ draftIds }),
  startWith: (exchangeId) => {
    const ids = get().draftIds.trim();
    set({ draftIds: ids ? `${ids}, ${exchangeId}` : String(exchangeId) });
    useAppStore.getState().setView("agent");
  },
  reset: () =>
    set({
      tasks: [],
      selected: null,
      activity: [],
      state: EMPTY_STATE,
      approvals: [],
      error: null,
    }),
}));
