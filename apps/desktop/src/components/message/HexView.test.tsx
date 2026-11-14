import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { HexView } from "./HexView";

const sendToDecoder = vi.fn();
vi.mock("../../state/tools", () => ({ sendToDecoder: (t: string) => sendToDecoder(t) }));
vi.mock("../../services/ipc", () => ({ api: {}, dialogs: {}, toIpcError: (e: unknown) => e }));
// jsdom has no layout: render every row.
vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 19,
    getVirtualItems: () =>
      Array.from({ length: count }, (_, i) => ({ index: i, key: i, start: i * 19 })),
  }),
}));

describe("HexView", () => {
  it("selects a range with click and Shift+click and sends it to the Decoder", () => {
    const bytes = new TextEncoder().encode("GET /secret HTTP/1.1\r\n");
    render(<HexView bytes={bytes} />);
    const cells = screen.getByLabelText("Hex dump").querySelectorAll(".hex-byte");
    const at = (i: number) => {
      const el = cells[i];
      if (!el) throw new Error(`no byte ${i}`);
      return el;
    };
    fireEvent.mouseDown(at(10)); // "t" of /secret
    fireEvent.mouseDown(at(4), { shiftKey: true }); // "/"
    expect(screen.getByText(/7 bytes selected/)).toBeTruthy();
    expect(at(6).classList.contains("hex-sel")).toBe(true);
    expect(at(11).classList.contains("hex-sel")).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: /Decode selection/ }));
    expect(sendToDecoder).toHaveBeenCalledWith("/secret");
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(screen.queryByText(/selected/)).toBeNull();
  });
});
