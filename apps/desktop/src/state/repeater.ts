// Repeater: tabs stored in the project, edited locally and saved shortly
// after each change, sent through the engine's Request Engine.
import {
  asRedirectPolicy,
  isRedirect,
  MAX_REDIRECTS,
  type RedirectPolicy,
  requestUrl,
  shouldFollow,
} from "../lib/redirects";
import { create } from "zustand";
import type { ExchangeDetail } from "../bindings/ExchangeDetail";
import type { Highlight } from "../bindings/Highlight";
import type { RepeaterGroup } from "../bindings/RepeaterGroup";
import type { RepeaterGroupUpdate } from "../bindings/RepeaterGroupUpdate";
import type { RepeaterSendRow } from "../bindings/RepeaterSendRow";
import type { RepeaterTab } from "../bindings/RepeaterTab";
import type { RepeaterTabUpdate } from "../bindings/RepeaterTabUpdate";
import type { RepeaterTarget } from "../bindings/RepeaterTarget";
import { HIGHLIGHTS } from "../lib/filters";
import { formatTarget, parseTarget, targetOfUrl } from "../lib/repeaterTarget";
import { api, toIpcError } from "../services/ipc";
import { useAppStore } from "./app";

/** Delay between the last edit and saving it to the project. */
const SAVE_DELAY_MS = 700;

/** What the response side of a tab shows. */
export type Shown =
  | { status: "idle" }
  | { status: "loading"; exchangeId: number }
  | { status: "ready"; send: RepeaterSendRow; detail: ExchangeDetail }
  | { status: "error"; message: string };

/** The local, possibly unsaved state of a tab. */
export interface Draft {
  name: string;
  targetText: string;
  request: string;
  updateContentLength: boolean;
  reuseConnection: boolean;
  useCookieJar: boolean;
}

export interface TabUi {
  draft: Draft;
  /** The draft differs from what is saved. */
  dirty: boolean;
  sending: boolean;
  shown: Shown;
  /** Sends, newest first (null until loaded). */
  history: RepeaterSendRow[] | null;
  /** The latest send, as far as known (shown on the tab). */
  latest: RepeaterSendRow | null;
  /** Last save or send problem with the tab itself. */
  problem: string | null;
}

interface RepeaterState {
  tabs: RepeaterTab[];
  groups: RepeaterGroup[];
  /** Groups being sent. */
  groupSending: Record<number, boolean>;
  loaded: boolean;
  error: string | null;
  activeId: number | null;
  ui: Record<number, TabUi>;

  load(): Promise<void>;
  select(id: number): void;
  /** Opens a new tab; resolves to its id (null on failure). */
  newTab(): Promise<number | null>;
  /** "Send to Repeater" for a stored request. */
  openExchange(exchangeId: number): Promise<void>;
  /** "Send to Repeater" for a request that is not stored (e.g. paused). */
  openRequest(url: string, request: string): Promise<void>;
  edit(id: number, patch: Partial<Draft>): void;
  /** Saves pending edits now. */
  flush(id: number): Promise<void>;
  duplicate(id: number): Promise<void>;
  close(id: number): Promise<void>;
  move(id: number, index: number): Promise<void>;
  send(id: number, hops?: number): Promise<void>;
  /** Sets when a tab follows redirects automatically (saved with the tab). */
  setRedirectPolicy(id: number, policy: RedirectPolicy): Promise<void>;
  cancel(id: number): Promise<void>;
  loadHistory(id: number): Promise<void>;
  /** Shows a past send's response. */
  show(id: number, send: RepeaterSendRow): Promise<void>;
  /** Puts a past send's request back into the editor. */
  restore(id: number, exchangeId: number): Promise<void>;
  /** Prepares the request that follows the redirect being shown. */
  followRedirect(id: number): Promise<void>;
  /** Groups tabs; resolves to the new group's id (null on failure). */
  createGroup(tabIds: number[], name?: string): Promise<number | null>;
  updateGroup(id: number, patch: Partial<RepeaterGroupUpdate>): Promise<void>;
  /** Removes a group; its tabs stay. */
  deleteGroup(id: number): Promise<void>;
  /** Moves a tab into a group, or out of its group with null. */
  setGroup(tabId: number, groupId: number | null): Promise<void>;
  /** Sets or clears a tab's highlight colour. */
  setColor(tabId: number, color: Highlight | null): Promise<void>;
  /** A tab dropped on another: it takes that place and that tab's group. */
  dropOnTab(tabId: number, targetId: number): Promise<void>;
  /** Sends every tab of a group (edits are saved first). */
  sendGroup(id: number): Promise<void>;
  cancelGroup(id: number): Promise<void>;
  reset(): void;
}

const timers = new Map<number, ReturnType<typeof setTimeout>>();

function draftOf(tab: RepeaterTab): Draft {
  return {
    name: tab.name,
    targetText: formatTarget(tab),
    request: tab.request,
    updateContentLength: tab.updateContentLength,
    reuseConnection: tab.reuseConnection,
    useCookieJar: tab.useCookieJar,
  };
}

function freshUi(tab: RepeaterTab): TabUi {
  return {
    draft: draftOf(tab),
    dirty: false,
    sending: false,
    shown: { status: "idle" },
    history: null,
    latest: null,
    problem: null,
  };
}

/** The update to save for a draft, or the reason it cannot be saved. */
export function updateOf(draft: Draft): RepeaterTabUpdate | { error: string } {
  const target = parseTarget(draft.targetText);
  if ("error" in target) return target;
  const name = draft.name.trim();
  if (!name) return { error: "The tab needs a name" };
  return {
    name,
    target,
    request: draft.request,
    updateContentLength: draft.updateContentLength,
    reuseConnection: draft.reuseConnection,
    useCookieJar: draft.useCookieJar,
  };
}

function sameDraft(a: Draft, b: Draft) {
  return (
    a.name.trim() === b.name.trim() &&
    a.request === b.request &&
    a.updateContentLength === b.updateContentLength &&
    a.reuseConnection === b.reuseConnection &&
    a.useCookieJar === b.useCookieJar &&
    a.targetText.trim() === b.targetText.trim()
  );
}

/** A colour no group uses yet (or the least used). */
function nextColor(groups: RepeaterGroup[]): Highlight {
  const used = (c: Highlight) => groups.filter((g) => g.color === c).length;
  return [...HIGHLIGHTS].sort((a, b) => used(a) - used(b))[0] ?? "blue";
}

/** "200, 200, 500" style summary of a group send. */
export function summarizeStatuses(statuses: (number | null)[]): string {
  const counts = new Map<string, number>();
  for (const s of statuses) {
    const key = s === null ? "no response" : String(s);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return [...counts].map(([k, n]) => (n > 1 ? `${n}× ${k}` : k)).join(", ");
}

/** A starting request for a target. */
function blankRequest(t: RepeaterTarget): string {
  const host = formatTarget(t).replace(/^https?:\/\//, "");
  return `GET / HTTP/1.1\nHost: ${host}\nUser-Agent: ChosProxy Repeater\nAccept: */*\nConnection: close\n\n`;
}

export const useRepeaterStore = create<RepeaterState>((set, get) => {
  const patchUi = (id: number, patch: Partial<TabUi>) =>
    set((s) => (s.ui[id] ? { ui: { ...s.ui, [id]: { ...s.ui[id], ...patch } } } : {}));

  const adopt = (tab: RepeaterTab, activate: boolean) =>
    set((s) => {
      const exists = s.tabs.some((t) => t.id === tab.id);
      const tabs = exists ? s.tabs.map((t) => (t.id === tab.id ? tab : t)) : [...s.tabs, tab];
      return {
        tabs: tabs.sort((a, b) => a.position - b.position || a.id - b.id),
        ui: s.ui[tab.id] ? s.ui : { ...s.ui, [tab.id]: freshUi(tab) },
        activeId: activate ? tab.id : s.activeId,
      };
    });

  const notifyError = (prefix: string, e: unknown) =>
    useAppStore.getState().notify("error", `${prefix}: ${toIpcError(e).message}`);

  const openTab = (tab: RepeaterTab) => {
    adopt(tab, true);
    useAppStore.getState().setView("repeater");
  };

  return {
    tabs: [],
    groups: [],
    groupSending: {},
    loaded: false,
    error: null,
    activeId: null,
    ui: {},

    async load() {
      try {
        const [tabs, groups] = await Promise.all([api.repeaterTabs(), api.repeaterGroups()]);
        set((s) => {
          const ui: Record<number, TabUi> = {};
          for (const t of tabs) ui[t.id] = s.ui[t.id] ?? freshUi(t);
          const activeId =
            s.activeId !== null && tabs.some((t) => t.id === s.activeId)
              ? s.activeId
              : (tabs[0]?.id ?? null);
          return { tabs, groups, ui, activeId, loaded: true, error: null };
        });
      } catch (e) {
        const err = toIpcError(e);
        set({ loaded: true, error: err.kind === "noProject" ? null : err.message });
      }
    },

    select(id) {
      set({ activeId: id });
      const ui = get().ui[id];
      if (ui?.history === null) void get().loadHistory(id);
      else if (ui?.shown.status === "idle" && ui.latest) void get().show(id, ui.latest);
    },

    async newTab() {
      const active = get().tabs.find((t) => t.id === get().activeId);
      const target: RepeaterTarget = active
        ? { scheme: active.scheme, host: active.host, port: active.port }
        : { scheme: "http", host: "localhost", port: 80 };
      try {
        const tab = await api.repeaterCreate({
          name: "",
          target,
          request: blankRequest(target),
          headCrlf: true,
          bodyCrlf: true,
        });
        adopt(tab, true);
        patchUi(tab.id, { history: [] });
        return tab.id;
      } catch (e) {
        notifyError("Could not open a Repeater tab", e);
        return null;
      }
    },

    async openExchange(exchangeId) {
      try {
        const tab = await api.repeaterFromExchange(exchangeId);
        openTab(tab);
        patchUi(tab.id, { history: [] });
      } catch (e) {
        notifyError("Could not send to Repeater", e);
      }
    },

    async openRequest(url, request) {
      const target = targetOfUrl(url);
      if (!target) {
        useAppStore.getState().notify("error", `Could not send to Repeater: invalid URL ${url}`);
        return;
      }
      try {
        const tab = await api.repeaterCreate({
          name: "",
          target,
          request,
          headCrlf: true,
          bodyCrlf: true,
        });
        openTab(tab);
        patchUi(tab.id, { history: [] });
      } catch (e) {
        notifyError("Could not send to Repeater", e);
      }
    },

    edit(id, patch) {
      const ui = get().ui[id];
      if (!ui) return;
      const draft = { ...ui.draft, ...patch };
      const tab = get().tabs.find((t) => t.id === id);
      patchUi(id, { draft, dirty: !tab || !sameDraft(draft, draftOf(tab)) });
      clearTimeout(timers.get(id));
      timers.set(
        id,
        setTimeout(() => void get().flush(id), SAVE_DELAY_MS),
      );
    },

    async flush(id) {
      clearTimeout(timers.get(id));
      timers.delete(id);
      const ui = get().ui[id];
      if (!ui || !ui.dirty) return;
      const update = updateOf(ui.draft);
      if ("error" in update) {
        // Invalid input stays local until it is fixed; the field shows why.
        return;
      }
      const saving = ui.draft;
      try {
        const tab = await api.repeaterUpdate(id, update);
        set((s) => ({ tabs: s.tabs.map((t) => (t.id === id ? tab : t)) }));
        const now = get().ui[id];
        if (now) patchUi(id, { dirty: !sameDraft(now.draft, saving), problem: null });
      } catch (e) {
        patchUi(id, { problem: `Not saved: ${toIpcError(e).message}` });
      }
    },

    async duplicate(id) {
      await get().flush(id);
      try {
        const tab = await api.repeaterDuplicate(id);
        // Positions after the original moved; reload the order.
        adopt(tab, true);
        patchUi(tab.id, { history: [] });
        await get().load();
      } catch (e) {
        notifyError("Could not duplicate the tab", e);
      }
    },

    async close(id) {
      clearTimeout(timers.get(id));
      timers.delete(id);
      try {
        await api.repeaterClose(id);
        set((s) => {
          const index = s.tabs.findIndex((t) => t.id === id);
          const tabs = s.tabs.filter((t) => t.id !== id);
          const ui = Object.fromEntries(Object.entries(s.ui).filter(([key]) => Number(key) !== id));
          const activeId =
            s.activeId === id ? (tabs[Math.min(index, tabs.length - 1)]?.id ?? null) : s.activeId;
          return { tabs, ui, activeId };
        });
      } catch (e) {
        notifyError("Could not close the tab", e);
      }
    },

    async move(id, index) {
      const tabs = [...get().tabs];
      const from = tabs.findIndex((t) => t.id === id);
      if (from < 0 || index < 0 || index >= tabs.length || from === index) return;
      const [moved] = tabs.splice(from, 1);
      if (!moved) return;
      tabs.splice(index, 0, moved);
      set({ tabs: tabs.map((t, i) => ({ ...t, position: i })) });
      try {
        await api.repeaterMove(id, index);
      } catch (e) {
        notifyError("Could not move the tab", e);
        await get().load();
      }
    },

    async setRedirectPolicy(id, policy) {
      try {
        const saved = await api.repeaterSetRedirectPolicy(id, policy);
        set((s) => ({
          tabs: s.tabs.map((t) =>
            t.id === id ? { ...t, redirectPolicy: saved.redirectPolicy } : t,
          ),
        }));
      } catch (e) {
        useAppStore.getState().notify("error", `Redirect setting: ${toIpcError(e).message}`);
      }
    },

    async send(id, hops = 0) {
      const ui = get().ui[id];
      if (!ui || ui.sending) return;
      const update = updateOf(ui.draft);
      if ("error" in update) {
        patchUi(id, { problem: update.error });
        return;
      }
      clearTimeout(timers.get(id));
      timers.delete(id);
      const sent = ui.draft;
      patchUi(id, { sending: true, problem: null });
      try {
        const result = await api.repeaterSend(id, update);
        set((s) => ({ tabs: s.tabs.map((t) => (t.id === id ? result.tab : t)) }));
        const now = get().ui[id];
        if (!now) return; // closed meanwhile
        const dirty = !sameDraft(now.draft, sent);
        patchUi(id, {
          sending: false,
          dirty,
          shown: { status: "ready", send: result.send, detail: result.detail },
          history: [result.send, ...(now.history ?? [])],
          latest: result.send,
        });
        // Edits made while sending are saved on top of what was sent.
        if (dirty) void get().flush(id);
        const policy = asRedirectPolicy(get().tabs.find((t) => t.id === id)?.redirectPolicy);
        if (policy !== "never" && !dirty && isRedirect(result.send.status)) {
          if (hops >= MAX_REDIRECTS) {
            useAppStore.getState().notify("info", `Stopped after ${MAX_REDIRECTS} redirects`);
            return;
          }
          const next = await api.repeaterRedirect(result.send.exchangeId);
          const from = requestUrl(formatTarget(result.tab), result.tab.request);
          const to = requestUrl(formatTarget(next.target), next.request);
          const ok = await shouldFollow(policy, from, to, () =>
            api.checkScope(to).then((d) => d.inScope),
          );
          if (!ok) {
            useAppStore.getState().notify("info", `Redirect to ${to} not followed (${policy})`);
            return;
          }
          get().edit(id, { request: next.request, targetText: formatTarget(next.target) });
          await get().send(id, hops + 1);
        }
      } catch (e) {
        const err = toIpcError(e);
        if (!get().ui[id]) return;
        patchUi(id, {
          sending: false,
          problem: err.kind === "cancelled" ? null : err.message,
        });
        if (err.kind === "cancelled") useAppStore.getState().notify("info", "Send cancelled");
      }
    },

    async cancel(id) {
      try {
        await api.repeaterCancel(id);
      } catch (e) {
        notifyError("Could not cancel", e);
      }
    },

    async loadHistory(id) {
      try {
        const history = await api.repeaterHistory(id);
        patchUi(id, { history, latest: history[0] ?? null });
        const ui = get().ui[id];
        if (ui && ui.shown.status === "idle" && history[0]) void get().show(id, history[0]);
      } catch (e) {
        patchUi(id, { history: [], problem: toIpcError(e).message });
      }
    },

    async show(id, send) {
      patchUi(id, { shown: { status: "loading", exchangeId: send.exchangeId } });
      try {
        const snapshot = await api.repeaterSnapshot(send.exchangeId);
        const ui = get().ui[id];
        if (ui?.shown.status === "loading" && ui.shown.exchangeId === send.exchangeId) {
          patchUi(id, { shown: { status: "ready", send, detail: snapshot.detail } });
        }
      } catch (e) {
        patchUi(id, { shown: { status: "error", message: toIpcError(e).message } });
      }
    },

    async restore(id, exchangeId) {
      try {
        const snapshot = await api.repeaterSnapshot(exchangeId);
        if (snapshot.requestText === null) {
          useAppStore
            .getState()
            .notify("error", "That request cannot be edited as text (binary body)");
          return;
        }
        const row = snapshot.detail.row;
        get().edit(id, {
          request: snapshot.requestText,
          targetText: formatTarget({ scheme: row.scheme, host: row.host, port: row.port }),
        });
      } catch (e) {
        notifyError("Could not restore the request", e);
      }
    },

    async followRedirect(id) {
      const shown = get().ui[id]?.shown;
      if (shown?.status !== "ready") return;
      try {
        const next = await api.repeaterRedirect(shown.send.exchangeId);
        get().edit(id, { request: next.request, targetText: formatTarget(next.target) });
        for (const note of next.notes) useAppStore.getState().notify("info", `Redirect: ${note}`);
        await get().send(id);
      } catch (e) {
        notifyError("Could not follow the redirect", e);
      }
    },

    async createGroup(tabIds, name) {
      const groups = get().groups;
      try {
        const group = await api.repeaterGroupCreate(
          name?.trim() || `Group ${groups.length + 1}`,
          nextColor(groups),
          tabIds,
        );
        await get().load();
        return group.id;
      } catch (e) {
        notifyError("Could not create the group", e);
        return null;
      }
    },

    async updateGroup(id, patch) {
      const group = get().groups.find((g) => g.id === id);
      if (!group) return;
      const update: RepeaterGroupUpdate = {
        name: patch.name ?? group.name,
        color: patch.color ?? group.color,
        sendMode: patch.sendMode ?? group.sendMode,
        collapsed: patch.collapsed ?? group.collapsed,
      };
      try {
        const saved = await api.repeaterGroupUpdate(id, update);
        set((s) => ({ groups: s.groups.map((g) => (g.id === id ? saved : g)) }));
      } catch (e) {
        notifyError("Could not change the group", e);
      }
    },

    async deleteGroup(id) {
      try {
        await api.repeaterGroupDelete(id);
        await get().load();
      } catch (e) {
        notifyError("Could not remove the group", e);
      }
    },

    async setGroup(tabId, groupId) {
      try {
        await api.repeaterSetGroup(tabId, groupId);
        await get().load();
      } catch (e) {
        notifyError("Could not move the tab", e);
      }
    },

    async setColor(tabId, color) {
      try {
        const saved = await api.repeaterSetColor(tabId, color);
        set({ tabs: get().tabs.map((t) => (t.id === tabId ? { ...t, color: saved.color } : t)) });
      } catch (e) {
        notifyError("Could not colour the tab", e);
      }
    },

    async dropOnTab(tabId, targetId) {
      if (tabId === targetId) return;
      const tabs = get().tabs;
      const tab = tabs.find((t) => t.id === tabId);
      const target = tabs.find((t) => t.id === targetId);
      const index = tabs.findIndex((t) => t.id === targetId);
      if (!tab || !target || index < 0) return;
      try {
        if (tab.groupId !== target.groupId) await api.repeaterSetGroup(tabId, target.groupId);
        await api.repeaterMove(tabId, index);
      } catch (e) {
        notifyError("Could not move the tab", e);
      }
      await get().load();
    },

    async sendGroup(id) {
      const group = get().groups.find((g) => g.id === id);
      if (!group || get().groupSending[id]) return;
      const members = get().tabs.filter((t) => t.groupId === id);
      if (members.length === 0) return;
      // The group sends what is saved: save every edit first.
      for (const t of members) {
        const ui = get().ui[t.id];
        if (!ui) continue;
        const update = updateOf(ui.draft);
        if ("error" in update) {
          patchUi(t.id, { problem: update.error });
          useAppStore
            .getState()
            .notify("error", `Group not sent: tab “${ui.draft.name}”: ${update.error}`);
          return;
        }
        await get().flush(t.id);
      }
      set((s) => ({ groupSending: { ...s.groupSending, [id]: true } }));
      for (const t of members) patchUi(t.id, { sending: true, problem: null });
      const { notify } = useAppStore.getState();
      try {
        const result = await api.repeaterSendGroup(id);
        for (const item of result.items) {
          const ui = get().ui[item.tabId];
          if (!ui) continue;
          if (item.send) {
            patchUi(item.tabId, {
              latest: item.send,
              history: ui.history ? [item.send, ...ui.history] : null,
              shown: { status: "idle" },
              problem: null,
            });
          } else {
            patchUi(item.tabId, { problem: item.problem });
          }
        }
        const sent = result.items.flatMap((i) => (i.send ? [i.send.status] : []));
        const skipped = result.items.length - sent.length;
        notify(
          skipped > 0 ? "warning" : "info",
          `Group “${group.name}” sent: ${summarizeStatuses(sent) || "nothing"}` +
            (skipped > 0 ? ` · ${skipped} not sent` : ""),
        );
      } catch (e) {
        const err = toIpcError(e);
        if (err.kind === "cancelled") {
          notify("info", "Group send cancelled; finished sends are in each tab's history");
          for (const t of members) patchUi(t.id, { history: null, shown: { status: "idle" } });
        } else {
          notify("error", `Could not send the group: ${err.message}`);
        }
      } finally {
        set((s) => ({ groupSending: { ...s.groupSending, [id]: false } }));
        for (const t of members) if (get().ui[t.id]) patchUi(t.id, { sending: false });
        const active = get().activeId;
        if (active !== null && members.some((t) => t.id === active)) {
          const ui = get().ui[active];
          if (ui?.history === null) void get().loadHistory(active);
          else if (ui?.latest) void get().show(active, ui.latest);
        }
      }
    },

    async cancelGroup(id) {
      try {
        await api.repeaterCancelGroup(id);
      } catch (e) {
        notifyError("Could not cancel", e);
      }
    },

    reset() {
      for (const t of timers.values()) clearTimeout(t);
      timers.clear();
      set({
        tabs: [],
        groups: [],
        groupSending: {},
        loaded: false,
        error: null,
        activeId: null,
        ui: {},
      });
    },
  };
});
