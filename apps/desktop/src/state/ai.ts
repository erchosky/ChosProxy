import { create } from "zustand";
import type { AiAttachment } from "../bindings/AiAttachment";
import type { AiTask } from "../bindings/AiTask";
import type { SessionRule } from "../bindings/SessionRule";

/** What the ChosAI dialog is about (M29). */
export interface AiSubject {
  /** Dialog title ("Request #12", "Finding #3"…). */
  title: string;
  attachments: AiAttachment[];
  /** The task selected when the dialog opens. */
  task: AiTask;
}

interface AiState {
  subject: AiSubject | null;
  open(subject: AiSubject): void;
  close(): void;
}

export const useAiStore = create<AiState>((set) => ({
  subject: null,
  open: (subject) => set({ subject }),
  close: () => set({ subject: null }),
}));

/** Tasks that make sense for what is attached, in menu order. */
export function tasksFor(attachments: AiAttachment[]): AiTask[] {
  const kinds = new Set(attachments.map((a) => a.kind));
  const out: AiTask[] = [];
  if (kinds.has("exchange"))
    out.push("explainExchange", "explainRequest", "explainResponse", "loginFlow");
  if (kinds.has("finding")) out.push("explainFinding", "draftFinding");
  if (kinds.has("text")) out.push("explainRequest", "explainJavascript");
  out.push("ask");
  return [...new Set(out)];
}

export const TASK_LABELS: Record<AiTask, string> = {
  explainRequest: "Explain the request",
  explainResponse: "Explain the response",
  explainExchange: "Explain request and response",
  explainFinding: "Explain the finding (confirm, fix)",
  explainJavascript: "Explain JavaScript",
  loginFlow: "Explain a login flow (session rule)",
  draftFinding: "Draft report text",
  ask: "Ask a question",
};

/** Exchange ids from "12, 13 15-17" (ranges up to 20 ids). */
export function parseIds(text: string): number[] {
  const out: number[] = [];
  for (const part of text.split(/[\s,]+/)) {
    if (part === "") continue;
    const range = /^#?(\d+)-#?(\d+)$/.exec(part);
    if (range) {
      const a = Number(range[1]);
      const b = Number(range[2]);
      for (let i = Math.min(a, b); i <= Math.max(a, b) && out.length < 20; i++) out.push(i);
      continue;
    }
    const one = /^#?(\d+)$/.exec(part);
    if (one) out.push(Number(one[1]));
  }
  return [...new Set(out)].slice(0, 20);
}

/**
 * The session rule a login-flow answer ends with (a fenced
 * `chosproxy-session-rule` JSON block), filled in with safe defaults; null
 * when there is none or it is not a rule.
 */
export function sessionRuleFrom(text: string): SessionRule | null {
  const m = /```chosproxy-session-rule\s*\n([\s\S]*?)```/.exec(text);
  if (!m) return null;
  let raw: unknown;
  try {
    raw = JSON.parse(m[1] ?? "");
  } catch {
    return null;
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return null;
  const r = raw as Record<string, unknown>;
  const strings = (v: unknown) =>
    Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  const numbers = (v: unknown) =>
    Array.isArray(v)
      ? v
          .map((x) => (typeof x === "string" ? Number(x.replace(/^#/, "")) : x))
          .filter((x): x is number => typeof x === "number" && Number.isInteger(x) && x > 0)
      : [];
  const pairs = <K extends string>(v: unknown, a: K, b: K) =>
    Array.isArray(v)
      ? v
          .filter(
            (x): x is Record<K, string> =>
              typeof x === "object" &&
              x !== null &&
              typeof (x as Record<string, unknown>)[a] === "string" &&
              typeof (x as Record<string, unknown>)[b] === "string",
          )
          .map((x) => ({ [a]: x[a], [b]: x[b] }) as Record<K, string>)
      : [];
  const name = typeof r.name === "string" && r.name.trim() ? r.name.trim() : "ChosAI session rule";
  return {
    enabled: true,
    name: name.slice(0, 80),
    hosts: strings(r.hosts),
    tools: strings(r.tools),
    useCookieJar: typeof r.useCookieJar === "boolean" ? r.useCookieJar : true,
    expiredStatuses: numbers(r.expiredStatuses).filter((s) => s >= 100 && s < 600),
    expiredRegex: typeof r.expiredRegex === "string" ? r.expiredRegex : "",
    macroRequests: numbers(r.macroRequests).slice(0, 10),
    extract: pairs(r.extract, "name", "regex") as { name: string; regex: string }[],
    headers: pairs(r.headers, "name", "value") as { name: string; value: string }[],
  };
}

interface SessionDraftState {
  /** A rule waiting to be added to Project → Session rules for review. */
  rule: SessionRule | null;
  offer(rule: SessionRule): void;
  take(): SessionRule | null;
}

/** Hands a suggested session rule to the session rules card. */
export const useSessionDraft = create<SessionDraftState>((set, get) => ({
  rule: null,
  offer: (rule) => set({ rule }),
  take: () => {
    const rule = get().rule;
    if (rule) set({ rule: null });
    return rule;
  },
}));

/** A name for one streamed answer (so its pieces and its cancel find it). */
export function newStreamId(): string {
  return `ai-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

/**
 * Collects the pieces of a streamed answer and hands the text so far to
 * `show` at most once per `ms`, so a fast model does not re-render the
 * dialog for every token. `end` shows what is left and stops.
 */
export function deltaBuffer(show: (text: string) => void, ms = 60) {
  let text = "";
  let timer: ReturnType<typeof setTimeout> | null = null;
  return {
    push(piece: string) {
      text += piece;
      timer ??= setTimeout(() => {
        timer = null;
        show(text);
      }, ms);
    },
    end(): string {
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
        show(text);
      }
      return text;
    },
  };
}
