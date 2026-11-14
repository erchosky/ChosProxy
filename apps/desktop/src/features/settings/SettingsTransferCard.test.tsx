import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { usePasswordStore } from "../../state/password";
import { SettingsTransferCard } from "./SettingsTransferCard";

const mocks = vi.hoisted(() => ({
  exportSettings: vi.fn(),
  previewSettingsImport: vi.fn(),
  applySettingsImport: vi.fn(),
  isSealedFile: vi.fn(),
  chooseSettingsDestination: vi.fn(),
  chooseSettingsFile: vi.fn(),
  chooseJsonDestination: vi.fn(),
  chooseJsonFile: vi.fn(),
  getSettings: vi.fn().mockResolvedValue({}),
  getProxyConfig: vi.fn().mockResolvedValue({ listeners: [] }),
  listListeners: vi.fn().mockResolvedValue([]),
  getScope: vi.fn().mockResolvedValue({ include: [], exclude: [] }),
}));

vi.mock("../../services/ipc", () => ({
  api: mocks,
  dialogs: mocks,
  toIpcError: (e: unknown) => ({
    kind: (e as { kind?: string })?.kind ?? "other",
    message: e instanceof Error ? e.message : String(e),
  }),
}));

describe("SettingsTransferCard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    act(() => {
      usePasswordStore.setState({ request: null, resolve: null });
    });
  });

  it("exports settings with a password and cancels cleanly when password dialog is dismissed", async () => {
    mocks.chooseSettingsDestination.mockResolvedValue("/tmp/out.chossettings");
    mocks.exportSettings.mockResolvedValue("/tmp/out.chossettings");

    render(<SettingsTransferCard />);

    fireEvent.click(screen.getByRole("button", { name: "Export…" }));
    await waitFor(() => {
      expect(usePasswordStore.getState().request?.title).toBe("Export settings");
    });

    act(() => {
      usePasswordStore.getState().answer({ encrypt: true, password: "my-settings-password" });
    });

    await waitFor(() => {
      expect(mocks.exportSettings).toHaveBeenCalledWith(
        "/tmp/out.chossettings",
        ["app", "proxy", "scope"],
        "my-settings-password",
      );
    });

    // Cancelled password dialog does not call exportSettings again.
    mocks.exportSettings.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Export…" }));
    await waitFor(() => {
      expect(usePasswordStore.getState().request).not.toBeNull();
    });
    act(() => {
      usePasswordStore.getState().answer(null);
    });
    expect(mocks.exportSettings).not.toHaveBeenCalled();
  });

  it("detects sealed settings file, prompts for password, previews, and applies with same password", async () => {
    mocks.chooseSettingsFile.mockResolvedValue("/tmp/in.chossettings");
    mocks.isSealedFile.mockResolvedValue(true);
    mocks.previewSettingsImport.mockResolvedValue({
      sections: [
        {
          section: "scope",
          valid: true,
          error: null,
          current: "{}",
          incoming: '{"include":[{"host":"example.com"}]}',
          changed: true,
        },
      ],
      warnings: [],
    });
    mocks.applySettingsImport.mockResolvedValue(["scope"]);

    render(<SettingsTransferCard />);

    fireEvent.click(screen.getByRole("button", { name: "Import…" }));
    await waitFor(() => {
      expect(usePasswordStore.getState().request?.mode).toBe("unlock");
    });

    act(() => {
      usePasswordStore.getState().answer({ encrypt: true, password: "unlock-pw" });
    });

    await waitFor(() => {
      expect(mocks.previewSettingsImport).toHaveBeenCalledWith("/tmp/in.chossettings", "unlock-pw");
    });

    const applyBtn = await screen.findByRole("button", { name: "Apply 1 section" });
    fireEvent.click(applyBtn);

    await waitFor(() => {
      expect(mocks.applySettingsImport).toHaveBeenCalledWith(
        "/tmp/in.chossettings",
        ["scope"],
        "unlock-pw",
      );
    });
  });
});
