import { beforeEach, describe, expect, it, vi } from "vitest";

const api = {
  findings: vi.fn(),
  passiveStatus: vi.fn(),
  passiveReanalyze: vi.fn(),
};
vi.mock("../services/ipc", () => ({
  api,
  dialogs: {},
  toIpcError: (e: unknown) => ({ kind: "unknown", message: String(e) }),
}));

const { useFindingsStore } = await import("./findings");

const status = {
  enabled: true,
  inScopeOnly: true,
  scopeDefined: false,
  analyzedUpTo: 10,
  lastExchangeId: 12,
  checksEnabled: 18,
  checksTotal: 18,
};

describe("findings store", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    useFindingsStore.getState().reset();
    api.findings.mockResolvedValue([]);
  });

  it("filters by source", async () => {
    await useFindingsStore.getState().load();
    expect(api.findings).toHaveBeenLastCalledWith(expect.objectContaining({ source: null }));
    useFindingsStore.getState().setFilter({ source: "passive" });
    await vi.waitFor(() =>
      expect(api.findings).toHaveBeenLastCalledWith(expect.objectContaining({ source: "passive" })),
    );
  });

  it("loads the passive status and starts a new analysis", async () => {
    api.passiveStatus.mockResolvedValue(status);
    await useFindingsStore.getState().loadPassive();
    expect(useFindingsStore.getState().passive?.lastExchangeId).toBe(12);
    api.passiveReanalyze.mockResolvedValue({ ...status, analyzedUpTo: 0 });
    await useFindingsStore.getState().reanalyze();
    expect(useFindingsStore.getState().passive?.analyzedUpTo).toBe(0);

    api.passiveStatus.mockRejectedValue(new Error("no project"));
    await useFindingsStore.getState().loadPassive();
    expect(useFindingsStore.getState().passive).toBeNull();
  });
});
