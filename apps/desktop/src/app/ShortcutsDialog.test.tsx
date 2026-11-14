import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useAppStore } from "../state/app";
import { ShortcutsDialog } from "./ShortcutsDialog";
import { useGlobalShortcuts } from "./useGlobalShortcuts";

vi.mock("../services/ipc", () => ({ api: {}, dialogs: {}, toIpcError: (e: unknown) => e }));

function Harness() {
  useGlobalShortcuts();
  return <ShortcutsDialog />;
}

describe("keyboard shortcuts overlay", () => {
  it("opens with ?, closes with Escape wherever focus is, and jumps to views", () => {
    render(<Harness />);
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.keyDown(document.body, { key: "?", shiftKey: true });
    expect(screen.getByRole("dialog", { name: "Keyboard shortcuts" })).toBeTruthy();
    expect(screen.getByText("Command palette")).toBeTruthy();

    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();

    fireEvent.keyDown(document.body, { key: "3", ctrlKey: true });
    expect(useAppStore.getState().view).toBe("history");
    act(() => useAppStore.getState().setView("dashboard"));
  });

  it("leaves ? to text fields", () => {
    render(
      <>
        <Harness />
        <input aria-label="field" />
      </>,
    );
    fireEvent.keyDown(screen.getByLabelText("field"), { key: "?", shiftKey: true });
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
