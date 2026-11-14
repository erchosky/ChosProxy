import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FindingRow } from "../bindings/FindingRow";

const findings = vi.fn();
vi.mock("../services/ipc", () => ({
  api: { findings: () => findings() },
  dialogs: {},
  toIpcError: (e: unknown) => ({ kind: "unknown", message: String(e) }),
}));

const { reportFileName, useReportStore } = await import("./report");

const row = (id: number, status: FindingRow["status"]) => ({ id, status }) as FindingRow;

describe("report store", () => {
  beforeEach(() => useReportStore.getState().reset());

  it("includes all but false positives, remembering explicit choices", async () => {
    findings.mockResolvedValue([row(1, "open"), row(2, "falsePositive"), row(3, "fixed")]);
    await useReportStore.getState().load();
    expect(useReportStore.getState().selected).toEqual([1, 3]);

    useReportStore.getState().setSelected([2, 3]);
    // Finding 1 is deleted and 4 is documented later: 4 is included by default.
    findings.mockResolvedValue([row(2, "falsePositive"), row(3, "fixed"), row(4, "open")]);
    await useReportStore.getState().load();
    expect(useReportStore.getState().selected).toEqual([2, 3, 4]);
    expect(useReportStore.getState().options().findingIds).toEqual([2, 3, 4]);

    useReportStore.getState().chooseDefault();
    expect(useReportStore.getState().selected).toEqual([3, 4]);
  });

  it("drops the answer of a load that a project change overtook", async () => {
    let answer: (rows: FindingRow[]) => void = () => {};
    findings.mockReturnValue(new Promise<FindingRow[]>((resolve) => (answer = resolve)));
    const loading = useReportStore.getState().load();
    useReportStore.getState().reset();
    answer([row(1, "open")]);
    await loading;
    expect(useReportStore.getState().selected).toBeNull();
    expect(useReportStore.getState().findings).toEqual([]);
  });

  it("names the file after the title in any language", () => {
    expect(reportFileName("  Informe de evaluación: API/v2  ")).toBe(
      "Informe-de-evaluación-API-v2",
    );
    expect(reportFileName("///")).toBe("report");
  });

  it("keeps the texts across projects", async () => {
    useReportStore.getState().patch({ author: "Tester" });
    useReportStore.getState().reset();
    expect(useReportStore.getState().form.author).toBe("Tester");
    expect(useReportStore.getState().selected).toBeNull();
  });
});
