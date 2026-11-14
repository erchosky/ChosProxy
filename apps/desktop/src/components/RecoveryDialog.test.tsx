import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useAppStore } from "../state/app";
import { RecoveryDialog } from "./RecoveryDialog";

const mocks = vi.hoisted(() => ({
  recoveryRestore: vi.fn(),
  recoveryOpenReadOnly: vi.fn(),
  recoveryDiscard: vi.fn(),
  recoveryStatus: vi.fn(),
  listRecentProjects: vi.fn().mockResolvedValue([]),
  scopeRules: vi.fn().mockResolvedValue([]),
  historyPage: vi.fn().mockResolvedValue({ items: [], total: 0 }),
  sitemapTree: vi.fn().mockResolvedValue([]),
  findingsList: vi.fn().mockResolvedValue([]),
  findingsCounts: vi.fn().mockResolvedValue({
    high: 0,
    medium: 0,
    low: 0,
    info: 0,
  }),
  tasksList: vi.fn().mockResolvedValue([]),
  poolList: vi.fn().mockResolvedValue([]),
  auditLog: vi.fn().mockResolvedValue([]),
  projectBackups: vi.fn().mockResolvedValue([]),
  getSettings: vi.fn().mockResolvedValue({}),
}));

vi.mock("../services/ipc", () => ({
  api: mocks,
  dialogs: {},
  toIpcError: (e: unknown) => ({
    kind: "other",
    message: e instanceof Error ? e.message : String(e),
  }),
}));

describe("RecoveryDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.recoveryStatus.mockResolvedValue(null);
    act(() => {
      useAppStore.setState({
        recovery: null,
      });
    });
  });

  it("renders interrupted tasks and supports Discard with temporary file deletion", async () => {
    act(() => {
      useAppStore.setState({
        recovery: {
          markerPath: "/tmp/sessions/s1.json",
          projectPath: "/tmp/demo.chosproxy",
          projectName: "demo",
          projectKind: "temporary",
          encrypted: false,
          ephemeralKey: false,
          fileExists: true,
          fileBytes: 8192,
          startedAt: 1700000000000,
          updatedAt: 1700000005000,
          pid: 12345,
          interruptedTasks: [
            { id: 1, kind: "crawl", name: "Crawl https://example.com" },
            { id: 2, kind: "live", name: "Live passive audit" },
          ],
          restorable: true,
          markerCorrupt: false,
          message: "Previous session exited unexpectedly while demo was open.",
        },
      });
    });

    render(<RecoveryDialog />);

    expect(screen.getByRole("dialog", { name: "Recover previous session" })).toBeTruthy();
    expect(screen.getByText("Crawl https://example.com")).toBeTruthy();
    expect(screen.getByText("Live passive audit")).toBeTruthy();

    const checkbox = screen.getByRole("switch");
    fireEvent.click(checkbox);
    mocks.recoveryDiscard.mockResolvedValue(undefined);
    fireEvent.click(screen.getByRole("button", { name: "Discard recovery" }));

    await waitFor(() => {
      expect(mocks.recoveryDiscard).toHaveBeenCalledWith(true);
    });
  });

  it("supports Restore and Open read-only actions", async () => {
    act(() => {
      useAppStore.setState({
        recovery: {
          markerPath: "/tmp/sessions/s2.json",
          projectPath: "/tmp/disk.chosproxy",
          projectName: "disk",
          projectKind: "persistent",
          encrypted: false,
          ephemeralKey: false,
          fileExists: true,
          fileBytes: 4096,
          startedAt: 1700000000000,
          updatedAt: 1700000005000,
          pid: 12345,
          interruptedTasks: [],
          restorable: true,
          markerCorrupt: false,
          message: "Previous session exited unexpectedly.",
        },
      });
    });

    mocks.recoveryOpenReadOnly.mockResolvedValue({
      name: "disk",
      path: "/tmp/disk.chosproxy",
      kind: "persistent",
      createdAt: 1700000000,
      schemaVersion: 1,
      sizeBytes: 4096,
      encrypted: false,
      readOnly: true,
    });

    render(<RecoveryDialog />);

    fireEvent.click(screen.getByRole("button", { name: "Open read-only" }));
    await waitFor(() => {
      expect(mocks.recoveryOpenReadOnly).toHaveBeenCalledWith(null);
    });
  });
});
