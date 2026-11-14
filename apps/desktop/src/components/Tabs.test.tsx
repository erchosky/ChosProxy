import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Tabs } from "./Tabs";

describe("Tabs", () => {
  const items = [
    { id: "a", label: "Alpha" },
    { id: "b", label: "Beta" },
    { id: "c", label: "Gamma", disabled: true },
  ] as const;

  it("marks the selected tab and changes on click", () => {
    const onChange = vi.fn();
    render(<Tabs items={items} value="a" onChange={onChange} label="Test tabs" />);
    expect(screen.getByRole("tab", { name: "Alpha" }).getAttribute("aria-selected")).toBe("true");
    fireEvent.click(screen.getByRole("tab", { name: "Beta" }));
    expect(onChange).toHaveBeenCalledWith("b");
  });

  it("moves with arrow keys, skipping disabled tabs", () => {
    const onChange = vi.fn();
    render(<Tabs items={items} value="b" onChange={onChange} label="Test tabs" />);
    fireEvent.keyDown(screen.getByRole("tablist"), { key: "ArrowRight" });
    expect(onChange).toHaveBeenCalledWith("a");
  });
});
