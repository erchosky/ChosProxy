import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { BarChart } from "./BarChart";

describe("BarChart", () => {
  const bars = [
    { label: "1", value: 4, detail: "4.00 bits" },
    { label: "2", value: 1, detail: "1.00 bits", flagged: true },
    { label: "3", value: 0, detail: "0 bits" },
    { label: "4", value: 0, detail: "0 bits", flagged: true },
  ];

  it("draws a mark per non-zero value, explains flags and reads out on hover", () => {
    const { container } = render(
      <BarChart
        title="Entropy per position"
        bars={bars}
        max={4}
        ticks={[0, 2, 4]}
        flaggedLabel="varies little"
        valueLabel="Entropy"
      />,
    );
    expect(screen.getByRole("img", { name: "Entropy per position" })).toBeTruthy();
    // Zero draws nothing, unless flagged (a stub on the baseline).
    expect(container.querySelectorAll(".bar-chart-bar")).toHaveLength(3);
    expect(container.querySelectorAll(".bar-chart-bar.flagged")).toHaveLength(2);
    expect(screen.getAllByText("varies little").length).toBeGreaterThan(0);

    const hits = container.querySelectorAll(".bar-chart-hit");
    expect(hits).toHaveLength(4);
    const second = hits[1];
    if (!second) throw new Error("missing hit target");
    fireEvent.mouseEnter(second);
    expect(container.querySelector(".bar-chart-readout")?.textContent).toBe("2: 1.00 bits");

    // The table view carries every value.
    const rows = container.querySelectorAll(".bar-chart-table tbody tr");
    expect(rows).toHaveLength(4);
    expect(rows[1]?.textContent).toContain("(varies little)");
  });

  it("has no legend when nothing is flagged", () => {
    render(
      <BarChart
        title="Lengths"
        bars={bars.filter((b) => !b.flagged)}
        max={4}
        ticks={[0, 4]}
        flaggedLabel="x"
        valueLabel="Tokens"
      />,
    );
    expect(screen.queryByText("x")).toBeNull();
  });
});
