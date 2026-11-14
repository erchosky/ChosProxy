import { describe, expect, it, vi } from "vitest";

vi.mock("../../services/ipc", () => ({ api: {}, dialogs: {}, toIpcError: (e: unknown) => e }));
const { localInput } = await import("./ScheduleControls");

describe("localInput", () => {
  it("formats local time for datetime-local and round-trips", () => {
    const at = new Date(2026, 10, 12, 9, 5).getTime();
    expect(localInput(at)).toBe("2026-11-12T09:05");
    expect(new Date(localInput(at)).getTime()).toBe(at);
  });
});
