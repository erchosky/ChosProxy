import { describe, expect, it, vi } from "vitest";
import { buildCommands, type CommandContext } from "./commands";

function ctx(overrides: Partial<CommandContext> = {}): CommandContext {
  return {
    project: null,
    listeners: [],
    theme: "dark",
    setView: vi.fn(),
    newTemporaryProject: vi.fn(),
    newEncryptedTemporaryProject: vi.fn(),
    createProject: vi.fn(),
    openProject: vi.fn(),
    closeProject: vi.fn(),
    saveProjectAs: vi.fn(),
    newCrawl: vi.fn(),
    newOastPayload: vi.fn(),
    startListener: vi.fn(),
    stopListener: vi.fn(),
    setTheme: vi.fn(),
    browserRunning: false,
    openBrowser: vi.fn(),
    closeBrowser: vi.fn(),
    caAvailable: true,
    exportCa: vi.fn(),
    interceptEnabled: false,
    setIntercept: vi.fn(),
    showShortcuts: vi.fn(),
    ...overrides,
  };
}

describe("buildCommands", () => {
  it("offers only project creation without a project", () => {
    const ids = buildCommands(ctx()).map((c) => c.id);
    expect(ids).toContain("new-temp");
    expect(ids).not.toContain("go-history");
    expect(ids).not.toContain("close-project");
  });

  it("offers navigation and listener toggles with a project", () => {
    const c = ctx({
      project: { name: "p", path: "/p", kind: "temporary", encrypted: false, readOnly: false },
      listeners: [
        {
          config: {
            id: "default",
            bind_address: "127.0.0.1",
            port: 8080,
            start_on_open: true,
            allow_remote: false,
            auth: null,
            invisible: false,
            invisible_tls_port: 443,
            http2: false,
            redirect: "",
            certificate: { kind: "shared" },
          },
          state: { state: "running", address: "127.0.0.1:8080" },
        },
      ],
    });
    const cmds = buildCommands(c);
    const toggle = cmds.find((x) => x.id === "listener-default");
    expect(toggle?.title).toMatch(/^Stop/);
    toggle?.run();
    expect(c.stopListener).toHaveBeenCalledWith("default");
    cmds.find((x) => x.id === "go-history")?.run();
    expect(c.setView).toHaveBeenCalledWith("history");
  });

  it("toggles the lab browser and exports the CA", () => {
    const project = {
      name: "p",
      path: "/p",
      kind: "temporary" as const,
      encrypted: false,
      readOnly: false,
    };
    const open = ctx({ project });
    buildCommands(open)
      .find((x) => x.id === "browser")
      ?.run();
    expect(open.openBrowser).toHaveBeenCalled();
    const running = ctx({ project, browserRunning: true });
    expect(buildCommands(running).find((x) => x.id === "browser")?.title).toBe("Close lab browser");
    expect(
      buildCommands(ctx({ project, caAvailable: false })).some((x) => x.id === "export-ca"),
    ).toBe(false);
  });

  it("toggles Intercept", () => {
    const c = ctx({
      project: { name: "p", path: "/p", kind: "temporary", encrypted: false, readOnly: false },
      interceptEnabled: true,
    });
    const cmd = buildCommands(c).find((x) => x.id === "toggle-intercept");
    expect(cmd?.title).toBe("Turn Intercept off");
    cmd?.run();
    expect(c.setIntercept).toHaveBeenCalledWith(false);
  });

  it("cycles the theme", () => {
    const c = ctx({ theme: "light" });
    buildCommands(c)
      .find((x) => x.id === "theme")
      ?.run();
    expect(c.setTheme).toHaveBeenCalledWith("system");
  });
});
