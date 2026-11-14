import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RepeaterSendRow } from "../bindings/RepeaterSendRow";
import type { RepeaterTab } from "../bindings/RepeaterTab";

const api = {
  repeaterTabs: vi.fn(),
  repeaterUpdate: vi.fn(),
  repeaterSend: vi.fn(),
  repeaterHistory: vi.fn(),
  repeaterSnapshot: vi.fn(),
  repeaterCreate: vi.fn(),
  repeaterGroups: vi.fn(),
  repeaterGroupCreate: vi.fn(),
  repeaterGroupUpdate: vi.fn(),
  repeaterSendGroup: vi.fn(),
  repeaterRedirect: vi.fn(),
  repeaterSetRedirectPolicy: vi.fn(),
  repeaterSetGroup: vi.fn(),
  repeaterSetColor: vi.fn(),
  repeaterMove: vi.fn(),
  checkScope: vi.fn(),
};
vi.mock("../services/ipc", () => ({
  api,
  dialogs: {},
  toIpcError: (e: unknown) =>
    typeof e === "object" && e !== null && "kind" in e
      ? (e as { kind: string; message: string })
      : { kind: "unknown", message: String(e) },
}));

const { summarizeStatuses, updateOf, useRepeaterStore } = await import("./repeater");
const { useAppStore } = await import("./app");

const tab = (patch: Partial<RepeaterTab> = {}): RepeaterTab => ({
  id: 1,
  position: 1,
  name: "1",
  scheme: "http",
  host: "lab.test",
  port: 8080,
  request: "GET / HTTP/1.1\nHost: lab.test:8080\n\n",
  headCrlf: true,
  bodyCrlf: true,
  updateContentLength: true,
  reuseConnection: false,
  useCookieJar: false,
  groupId: null,
  color: null,
  createdAt: 0,
  updatedAt: 0,
  note: "",
  redirectPolicy: "never",
  ...patch,
});

function ui1() {
  const ui = useRepeaterStore.getState().ui[1];
  if (!ui) throw new Error("tab 1 is not loaded");
  return ui;
}

const sendRow = (id: number): RepeaterSendRow => ({
  id,
  tabId: 1,
  exchangeId: 100 + id,
  sentAt: 0,
  method: "GET",
  url: "http://lab.test:8080/",
  status: 200,
  responseLen: 10,
  error: null,
  timing: { dnsMs: 0, connectMs: 0, tlsMs: 0, waitMs: 1, downloadMs: 0, totalMs: 2 },
  reused: false,
  jarCookies: [],
});

describe("repeater store", () => {
  beforeEach(async () => {
    vi.useRealTimers();
    for (const fn of Object.values(api)) fn.mockReset();
    useRepeaterStore.getState().reset();
    api.repeaterTabs.mockResolvedValue([tab()]);
    api.repeaterGroups.mockResolvedValue([]);
    await useRepeaterStore.getState().load();
  });

  it("checks drafts before saving", () => {
    const draft = ui1().draft;
    expect(draft.targetText).toBe("http://lab.test:8080");
    expect(updateOf(draft)).toEqual({
      name: "1",
      target: { scheme: "http", host: "lab.test", port: 8080 },
      request: draft.request,
      updateContentLength: true,
      reuseConnection: false,
      useCookieJar: false,
    });
    expect(updateOf({ ...draft, targetText: "ftp://x" })).toHaveProperty("error");
    expect(updateOf({ ...draft, name: " " })).toHaveProperty("error");
  });

  it("saves edits after a pause, and only valid ones", async () => {
    vi.useFakeTimers();
    api.repeaterUpdate.mockImplementation((_id: number, u: { request: string }) =>
      Promise.resolve(tab({ request: u.request })),
    );
    const { edit } = useRepeaterStore.getState();
    edit(1, { targetText: "not a port:x" });
    expect(ui1().dirty).toBe(true);
    await vi.runAllTimersAsync();
    expect(api.repeaterUpdate).not.toHaveBeenCalled();

    edit(1, { targetText: "http://lab.test:8080", request: "POST / HTTP/1.1\n\nx" });
    edit(1, { request: "POST / HTTP/1.1\n\nxy" });
    await vi.runAllTimersAsync();
    expect(api.repeaterUpdate).toHaveBeenCalledTimes(1);
    expect(api.repeaterUpdate.mock.calls[0]?.[1].request).toBe("POST / HTTP/1.1\n\nxy");
    expect(ui1().dirty).toBe(false);
  });

  it("sends, shows the response and keeps the history newest first", async () => {
    api.repeaterHistory.mockResolvedValue([sendRow(1)]);
    api.repeaterSnapshot.mockResolvedValue({ detail: { row: { id: 101 } }, requestText: null });
    await useRepeaterStore.getState().loadHistory(1);
    await vi.waitFor(() => expect(ui1().shown.status).toBe("ready"));

    api.repeaterSend.mockResolvedValue({
      tab: tab(),
      send: sendRow(2),
      detail: { row: { id: 102 } },
    });
    await useRepeaterStore.getState().send(1);
    const ui = ui1();
    expect(ui.sending).toBe(false);
    expect((ui.history ?? []).map((h) => h.id)).toEqual([2, 1]);
    expect(ui.shown.status === "ready" && ui.shown.send.id).toBe(2);
  });

  it("follows redirects on the same site and stops at another site", async () => {
    api.repeaterSetRedirectPolicy.mockResolvedValue(tab({ redirectPolicy: "onSite" }));
    await useRepeaterStore.getState().setRedirectPolicy(1, "onSite");
    expect(useRepeaterStore.getState().tabs[0]?.redirectPolicy).toBe("onSite");
    api.repeaterSend
      .mockResolvedValueOnce({
        tab: tab({ redirectPolicy: "onSite" }),
        send: { ...sendRow(2), status: 302 },
        detail: { row: { id: 102 } },
      })
      .mockResolvedValueOnce({
        tab: tab({
          request: "GET /next HTTP/1.1\nHost: lab.test:8080\n\n",
          redirectPolicy: "onSite",
        }),
        send: { ...sendRow(3), status: 301 },
        detail: { row: { id: 103 } },
      });
    api.repeaterRedirect
      .mockResolvedValueOnce({
        target: { scheme: "http", host: "lab.test", port: 8080 },
        request: "GET /next HTTP/1.1\nHost: lab.test:8080\n\n",
        notes: [],
      })
      .mockResolvedValueOnce({
        target: { scheme: "https", host: "elsewhere.test", port: 443 },
        request: "GET / HTTP/1.1\nHost: elsewhere.test\n\n",
        notes: [],
      });
    await useRepeaterStore.getState().send(1);
    expect(api.repeaterSend).toHaveBeenCalledTimes(2);
    expect(api.repeaterSend.mock.calls[1]?.[1]).toMatchObject({
      request: expect.stringContaining("/next"),
    });
    expect(ui1().latest?.id).toBe(3);
  });

  it("treats a cancelled send as no problem", async () => {
    const notify = vi.spyOn(useAppStore.getState(), "notify");
    api.repeaterSend.mockRejectedValue({ kind: "cancelled", message: "cancelled" });
    await useRepeaterStore.getState().send(1);
    expect(ui1().problem).toBeNull();
    expect(notify).toHaveBeenCalledWith("info", "Send cancelled");

    api.repeaterSend.mockRejectedValue({ kind: "invalid", message: "the request is empty" });
    await useRepeaterStore.getState().send(1);
    expect(ui1().problem).toBe("the request is empty");
  });

  it("refuses to send to an invalid target", async () => {
    useRepeaterStore.getState().edit(1, { targetText: "ftp://x" });
    await useRepeaterStore.getState().send(1);
    expect(api.repeaterSend).not.toHaveBeenCalled();
    expect(ui1().problem).toMatch(/http or https/);
  });
});

describe("repeater groups", () => {
  const group = {
    id: 7,
    position: 1,
    name: "Race",
    color: "red" as const,
    sendMode: "parallel" as const,
    collapsed: false,
    createdAt: 0,
  };

  beforeEach(async () => {
    for (const fn of Object.values(api)) fn.mockReset();
    useRepeaterStore.getState().reset();
    api.repeaterTabs.mockResolvedValue([
      tab({ id: 1, groupId: 7 }),
      tab({ id: 2, name: "2", position: 2, groupId: 7 }),
      tab({ id: 3, name: "3", position: 3 }),
    ]);
    api.repeaterGroups.mockResolvedValue([group]);
    await useRepeaterStore.getState().load();
  });

  it("drops a tab on another: its place and its group", async () => {
    api.repeaterSetGroup.mockResolvedValue(tab({ id: 3, groupId: 7 }));
    api.repeaterMove.mockResolvedValue(null);
    await useRepeaterStore.getState().dropOnTab(3, 1);
    expect(api.repeaterSetGroup).toHaveBeenCalledWith(3, 7);
    expect(api.repeaterMove).toHaveBeenCalledWith(3, 0);
    // Inside the same group only the place changes; on itself, nothing.
    api.repeaterSetGroup.mockClear();
    api.repeaterMove.mockClear();
    await useRepeaterStore.getState().dropOnTab(2, 1);
    expect(api.repeaterSetGroup).not.toHaveBeenCalled();
    expect(api.repeaterMove).toHaveBeenCalledWith(2, 0);
    api.repeaterMove.mockClear();
    await useRepeaterStore.getState().dropOnTab(1, 1);
    expect(api.repeaterMove).not.toHaveBeenCalled();
  });

  it("colours a tab without reloading the others", async () => {
    api.repeaterSetColor.mockResolvedValue(tab({ id: 3, color: "green" }));
    await useRepeaterStore.getState().setColor(3, "green");
    const tabs = useRepeaterStore.getState().tabs;
    expect(tabs.find((t) => t.id === 3)?.color).toBe("green");
    expect(tabs.find((t) => t.id === 1)?.color).toBeNull();
  });

  it("summarizes statuses", () => {
    expect(summarizeStatuses([200, 200, 500, null])).toBe("2× 200, 500, no response");
    expect(summarizeStatuses([])).toBe("");
  });

  it("sends the group and shows each tab's latest status", async () => {
    const notify = vi.spyOn(useAppStore.getState(), "notify");
    api.repeaterSendGroup.mockResolvedValue({
      groupId: 7,
      mode: "parallel",
      items: [
        { tabId: 1, send: { ...sendRow(5), tabId: 1 }, problem: null },
        { tabId: 2, send: null, problem: "not sent: the connection was closed" },
      ],
    });
    api.repeaterHistory.mockResolvedValue([{ ...sendRow(5), tabId: 1 }]);
    api.repeaterSnapshot.mockResolvedValue({ detail: { row: { id: 105 } }, requestText: null });
    await useRepeaterStore.getState().sendGroup(7);
    expect(api.repeaterSendGroup).toHaveBeenCalledWith(7);
    const { ui, groupSending } = useRepeaterStore.getState();
    expect(groupSending[7]).toBe(false);
    expect(ui[1]?.latest?.id).toBe(5);
    expect(ui[1]?.sending).toBe(false);
    expect(ui[2]?.problem).toMatch(/closed/);
    expect(ui[3]?.latest).toBeNull();
    expect(notify).toHaveBeenCalledWith("warning", "Group “Race” sent: 200 · 1 not sent");
  });

  it("does not send a group with an invalid tab", async () => {
    useRepeaterStore.getState().edit(2, { targetText: "ftp://x" });
    await useRepeaterStore.getState().sendGroup(7);
    expect(api.repeaterSendGroup).not.toHaveBeenCalled();
    expect(useRepeaterStore.getState().ui[2]?.problem).toMatch(/http or https/);
  });

  it("creates groups with a free colour", async () => {
    api.repeaterGroupCreate.mockResolvedValue({ ...group, id: 8 });
    expect(await useRepeaterStore.getState().createGroup([3])).toBe(8);
    expect(api.repeaterGroupCreate).toHaveBeenCalledWith("Group 2", "orange", [3]);
  });

  it("collapses a group and keeps its other settings", async () => {
    api.repeaterGroupUpdate.mockResolvedValue({ ...group, collapsed: true });
    await useRepeaterStore.getState().updateGroup(7, { collapsed: true });
    expect(api.repeaterGroupUpdate).toHaveBeenCalledWith(7, {
      name: "Race",
      color: "red",
      sendMode: "parallel",
      collapsed: true,
    });
    expect(useRepeaterStore.getState().groups[0]?.collapsed).toBe(true);
  });
});
