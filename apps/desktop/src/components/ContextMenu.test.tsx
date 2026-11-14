import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SEPARATOR, useContextMenuStore } from "../state/contextMenu";
import { ContextMenu } from "./ContextMenu";

describe("ContextMenu", () => {
  beforeEach(() => act(() => useContextMenuStore.getState().close()));

  function open(onSend = vi.fn(), onToggle = vi.fn()) {
    render(<ContextMenu />);
    act(() =>
      useContextMenuStore
        .getState()
        .open(10, 10, "Request actions", [
          { heading: "Request #1" },
          { label: "Send to Repeater", onSelect: onSend },
          { label: "Disabled", onSelect: vi.fn(), disabled: true },
          SEPARATOR,
          { label: "Keep-alive", checked: true, onSelect: onToggle },
        ]),
    );
  }

  it("shows items, runs the chosen one and closes", () => {
    const onSend = vi.fn();
    open(onSend);
    expect(screen.getByRole("menu", { name: "Request actions" })).toBeTruthy();
    expect(
      screen.getByRole("menuitemcheckbox", { name: "Keep-alive" }).getAttribute("aria-checked"),
    ).toBe("true");
    fireEvent.click(screen.getByRole("menuitem", { name: "Send to Repeater" }));
    expect(onSend).toHaveBeenCalledOnce();
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("moves with the arrow keys over enabled items and closes on Escape", () => {
    open();
    const menu = screen.getByRole("menu");
    expect(document.activeElement?.textContent).toContain("Send to Repeater");
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement?.textContent).toContain("Keep-alive");
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement?.textContent).toContain("Send to Repeater");
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("closes on a click outside", () => {
    open();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByRole("menu")).toBeNull();
  });
});
