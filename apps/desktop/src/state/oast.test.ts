import { beforeEach, describe, expect, it, vi } from "vitest";
import type { OastPayloadRow } from "../bindings/OastPayloadRow";

const api = vi.hoisted(() => ({
  oastNewPayload: vi.fn(),
  oastInteractions: vi.fn(),
  oastInteraction: vi.fn(),
}));
const copied = vi.hoisted(() => ({ text: "" }));
vi.mock("../services/ipc", () => ({ api, dialogs: {}, toIpcError: (e: unknown) => e }));
vi.mock("../services/clipboard", () => ({
  copyText: (t: string) => {
    copied.text = t;
    return Promise.resolve(true);
  },
}));

const { useOastStore } = await import("./oast");

const payload: OastPayloadRow = {
  id: 1,
  correlationId: "abcdefghijklmnopqrst",
  createdAt: 1,
  note: "",
  dnsName: "abcdefghijklmnopqrst.oast.example",
  url: "http://abcdefghijklmnopqrst.oast.example/",
  interactions: 0,
  requests: 0,
  lastInteraction: null,
};

describe("oast store", () => {
  beforeEach(() => {
    useOastStore.setState({ payloads: [], interactions: [], filterId: null });
    vi.clearAllMocks();
  });

  it("creates a payload, lists it first and copies what was asked", async () => {
    api.oastNewPayload.mockResolvedValue(payload);
    await useOastStore.getState().newPayload("note", "dns");
    expect(api.oastNewPayload).toHaveBeenCalledWith("note");
    expect(useOastStore.getState().payloads[0]?.id).toBe(1);
    expect(copied.text).toBe("abcdefghijklmnopqrst.oast.example");
  });

  it("filters interactions by identifier and ignores stale details", async () => {
    api.oastInteractions.mockResolvedValue([]);
    await useOastStore.getState().setFilter(payload.correlationId);
    expect(api.oastInteractions).toHaveBeenCalledWith(payload.correlationId);
    expect(useOastStore.getState().filterId).toBe(payload.correlationId);

    let resolveFirst: (v: unknown) => void = () => {};
    api.oastInteraction
      .mockReturnValueOnce(new Promise((r) => (resolveFirst = r)))
      .mockResolvedValueOnce({ row: { id: 2 } });
    useOastStore.getState().selectInteraction(1);
    useOastStore.getState().selectInteraction(2);
    await vi.waitFor(() => expect(useOastStore.getState().detail.status).toBe("ready"));
    resolveFirst({ row: { id: 1 } });
    await Promise.resolve();
    const d = useOastStore.getState().detail;
    expect(d.status === "ready" && d.id).toBe(2);
  });
});
