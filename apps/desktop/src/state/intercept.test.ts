import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HeldDetail } from "../bindings/HeldDetail";

const api = {
  interceptState: vi.fn(),
  setIntercept: vi.fn(),
  heldMessage: vi.fn(),
  unpackHeld: vi.fn(),
  forwardHeld: vi.fn(),
  dropHeld: vi.fn(),
  forwardAllHeld: vi.fn(),
};
vi.mock("../services/ipc", () => ({
  api,
  toIpcError: (e: unknown) => ({ kind: "unknown", message: String(e) }),
}));
vi.mock("./app", () => ({ useAppStore: { getState: () => ({ notify: vi.fn() }) } }));

const { useInterceptStore } = await import("./intercept");

const detail = (id: number, text: string | null): HeldDetail => ({
  summary: {
    id,
    kind: "request",
    heldAt: 0,
    method: "POST",
    url: "http://lab/echo",
    host: "lab",
    status: null,
    size: 10,
    wsOpcode: null,
  },
  rawBase64: "",
  text,
  notEditable: text === null ? "binary" : null,
  packed: false,
  request: null,
});

describe("intercept store", () => {
  beforeEach(() => {
    Object.values(api).forEach((f) => f.mockReset());
    api.interceptState.mockResolvedValue({ enabled: true, pending: [] });
    useInterceptStore.setState({
      selectedId: 7,
      detail: detail(7, "POST / HTTP/1.1\n\na=1"),
      draft: null,
      updateContentLength: true,
    });
  });

  it("forwards unchanged messages without an edit", async () => {
    await useInterceptStore.getState().forward(false);
    expect(api.forwardHeld).toHaveBeenCalledWith(7, null, false);
  });

  it("forwards the edited text with the Content-Length choice", async () => {
    const s = useInterceptStore.getState();
    s.setDraft("POST / HTTP/1.1\n\na=2");
    s.setUpdateContentLength(false);
    await useInterceptStore.getState().forward(true);
    expect(api.forwardHeld).toHaveBeenCalledWith(
      7,
      { text: "POST / HTTP/1.1\n\na=2", updateContentLength: false },
      true,
    );
  });

  it("treats a draft equal to the original as unchanged", () => {
    useInterceptStore.getState().setDraft("POST / HTTP/1.1\n\na=1");
    expect(useInterceptStore.getState().draft).toBeNull();
  });

  it("selects the next paused message after a decision", async () => {
    api.interceptState.mockResolvedValue({ enabled: true, pending: [detail(9, "x").summary] });
    api.heldMessage.mockResolvedValue(detail(9, "x"));
    await useInterceptStore.getState().drop();
    expect(api.dropHeld).toHaveBeenCalledWith(7);
    expect(useInterceptStore.getState().selectedId).toBe(9);
  });
});
