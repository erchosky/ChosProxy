import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useOrganizerStore } from "../../state/organizer";
import { usePasswordStore } from "../../state/password";
import { OrganizerView } from "./OrganizerView";

const mocks = vi.hoisted(() => ({
  organizerList: vi.fn(),
  organizerDetail: vi.fn(),
  organizerUpdate: vi.fn(),
  organizerDelete: vi.fn(),
  organizerExport: vi.fn(),
  organizerImport: vi.fn(),
  isSealedFile: vi.fn(),
  chooseOrganizerDestination: vi.fn(),
  chooseOrganizerFile: vi.fn(),
  chooseJsonDestination: vi.fn(),
  chooseJsonFile: vi.fn(),
}));

vi.mock("../../services/ipc", () => ({
  api: mocks,
  dialogs: mocks,
  toIpcError: (e: unknown) => ({
    kind: (e as { kind?: string })?.kind ?? "other",
    message: e instanceof Error ? e.message : String(e),
  }),
}));

describe("OrganizerView export & import", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.organizerList.mockResolvedValue([
      {
        id: 1,
        createdAt: 1700000000000,
        updatedAt: 1700000001000,
        exchangeId: 10,
        name: "POST /login",
        notes: "Auth request",
        tags: ["auth"],
        status: "confirmed",
        severity: "high",
        method: "POST",
        url: "https://example.com/login",
        statusCode: 200,
        collection: "Auth/Login",
      },
    ]);
    act(() => {
      useOrganizerStore.getState().reset();
      usePasswordStore.setState({ request: null, resolve: null });
    });
  });

  it("exports selected collection with password and imports sealed file", async () => {
    mocks.chooseOrganizerDestination.mockResolvedValue("/tmp/auth.chosorg");
    mocks.organizerExport.mockResolvedValue({ items: 1, encrypted: true });

    render(<OrganizerView />);

    await waitFor(() => {
      expect(screen.getByText("POST /login")).toBeTruthy();
    });

    // Select parent collection "Auth" in tree
    fireEvent.click(screen.getByRole("button", { name: /^Auth\s/ }));
    const exportBtn = screen.getByRole("button", { name: "Export “Auth”…" });
    fireEvent.click(exportBtn);

    await waitFor(() => {
      expect(usePasswordStore.getState().request?.mode).toBe("choose");
    });

    act(() => {
      usePasswordStore.getState().answer({ encrypt: true, password: "org-export-password" });
    });

    await waitFor(() => {
      expect(mocks.organizerExport).toHaveBeenCalledWith(
        "/tmp/auth.chosorg",
        "Auth",
        "org-export-password",
      );
    });

    // Import a sealed Organizer file
    mocks.chooseOrganizerFile.mockResolvedValue("/tmp/auth.chosorg");
    mocks.isSealedFile.mockResolvedValue(true);
    mocks.organizerImport.mockResolvedValue({ items: 1, encrypted: true });

    fireEvent.click(screen.getByRole("button", { name: "Import…" }));
    await waitFor(() => {
      expect(usePasswordStore.getState().request?.mode).toBe("unlock");
    });

    act(() => {
      usePasswordStore.getState().answer({ encrypt: true, password: "org-export-password" });
    });

    await waitFor(() => {
      expect(mocks.organizerImport).toHaveBeenCalledWith(
        "/tmp/auth.chosorg",
        "org-export-password",
      );
    });
  });
});
