import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HistoryQuery } from "../bindings/HistoryQuery";
import type { HistoryRow } from "../bindings/HistoryRow";

const historyPage = vi.fn();
vi.mock("../services/ipc", () => ({
  api: { historyPage: (q: HistoryQuery) => historyPage(q), exchangeDetail: vi.fn() },
  toIpcError: (e: unknown) => ({ kind: "unknown", message: String(e) }),
}));

const { PAGE_SIZE, useHistoryStore, useLoggerStore } = await import("./history");

function row(id: number): HistoryRow {
  return { id } as HistoryRow;
}

async function settle() {
  await new Promise((r) => setTimeout(r, 0));
}

describe("history store", () => {
  it("limits HTTP history to proxy traffic but not the Logger", async () => {
    historyPage.mockResolvedValue({ total: 0, rows: [] });
    useHistoryStore.getState().invalidate();
    useLoggerStore.getState().invalidate();
    await settle();
    expect(historyPage.mock.calls[0]?.[0].sources).toEqual(["proxy"]);
    expect(historyPage.mock.calls[1]?.[0].sources).toEqual([]);
    expect(historyPage.mock.calls[1]?.[0].order).toBe("asc");
  });

  beforeEach(() => {
    historyPage.mockReset();
    useHistoryStore.setState({ pages: {}, total: 0, selectedId: null });
  });

  it("reloads after reset, e.g. when another project is opened", async () => {
    historyPage.mockResolvedValue({ total: 2, rows: [row(2), row(1)] });
    useHistoryStore.getState().reset();
    await settle();
    expect(historyPage).toHaveBeenCalledTimes(1);
    expect(useHistoryStore.getState().total).toBe(2);
  });

  it("finds rows by id across pages", async () => {
    useHistoryStore.setState({ pages: { 0: [row(9), row(8)], 1: [row(3)] } });
    const { indexOfId } = useHistoryStore.getState();
    expect(indexOfId(8)).toBe(1);
    expect(indexOfId(3)).toBe(PAGE_SIZE);
    expect(indexOfId(42)).toBeNull();
  });

  it("ignores responses from a stale generation", async () => {
    let resolveFirst: (v: unknown) => void = () => {};
    historyPage
      .mockReturnValueOnce(new Promise((r) => (resolveFirst = r)))
      .mockResolvedValueOnce({ total: 1, rows: [row(5)] });
    useHistoryStore.getState().invalidate();
    useHistoryStore.getState().setFilter({ search: "x" });
    await settle();
    resolveFirst({ total: 99, rows: [row(1)] });
    await settle();
    expect(useHistoryStore.getState().total).toBe(1);
  });
});

describe("page pruning", () => {
  it("keeps only pages near the visible range", async () => {
    const { prunePages, KEEP_PAGES } = await import("./history");
    const pages: Record<number, string> = {};
    for (let p = 0; p < 40; p++) pages[p] = `p${p}`;
    const kept = prunePages(pages, 20, 21);
    expect(Object.keys(kept).map(Number)).toEqual(
      Array.from({ length: 2 + 2 * KEEP_PAGES }, (_, i) => 20 - KEEP_PAGES + i),
    );
  });
});
