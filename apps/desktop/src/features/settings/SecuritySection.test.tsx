import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { useProjectStore } from "../../state/project";
import { SecuritySection } from "./SecuritySection";

describe("SecuritySection & audit filtering", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    useAppStore.setState({
      view: "settings",
      project: {
        name: "Test",
        path: "/tmp/test.chosproxy",
        kind: "persistent",
        encrypted: true,
        readOnly: false,
      },
    });
    useProjectStore.getState().reset();
  });

  afterEach(() => {
    cleanup();
  });

  it("renders security checks and navigates to targetView on Review", async () => {
    vi.spyOn(api, "securityOverview").mockResolvedValue({
      okCount: 10,
      warnCount: 2,
      infoCount: 1,
      unavailableCount: 0,
      checks: [
        {
          id: "ca_certificate",
          title: "Root CA & private key",
          status: "ok",
          summary: "Per-installation Root CA ready",
          detail: "SHA-256 ABCD",
          targetView: "https",
          targetSection: null,
        },
        {
          id: "proxy_listeners",
          title: "Proxy listeners & bind exposure",
          status: "warn",
          summary: "1 listener(s) bound beyond loopback without authentication",
          detail: "Unauthenticated non-loopback listeners: 0.0.0.0:8080.",
          targetView: "listeners",
          targetSection: null,
        },
      ],
    });

    render(<SecuritySection />);

    await waitFor(() => {
      expect(screen.getByText("10 OK")).toBeTruthy();
      expect(screen.getByText("2 Warning")).toBeTruthy();
    });
    expect(screen.getByText("Root CA & private key")).toBeTruthy();
    expect(
      screen.getByText("1 listener(s) bound beyond loopback without authentication"),
    ).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Review Proxy listeners & bind exposure" }));
    expect(useAppStore.getState().view).toBe("listeners");
  });

  it("queries filtered audit entries via useProjectStore.queryAudit", async () => {
    const auditLogSpy = vi.spyOn(api, "auditLog").mockResolvedValue([
      {
        id: 1,
        at: 1700000000000,
        action: "task.crawl.started",
        detail: "task #1 (Crawl)",
      },
    ]);

    await useProjectStore.getState().queryAudit({
      actionPrefix: "task.",
      search: "Crawl",
    });

    expect(auditLogSpy).toHaveBeenCalledWith({
      actionPrefix: "task.",
      search: "Crawl",
      fromMs: null,
      toMs: null,
      limit: 200,
      offset: 0,
    });
    expect(useProjectStore.getState().audit).toHaveLength(1);
    expect(useProjectStore.getState().audit[0]?.action).toBe("task.crawl.started");
  });
});
