import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PluginInfo } from "../bindings/PluginInfo";

const api = {
  pluginsTrustSettings: vi.fn(),
  plugins: vi.fn(),
  pluginsDir: vi.fn(),
  pluginSet: vi.fn(),
  pluginLogs: vi.fn(),
  pluginsInstallExample: vi.fn(),
};
vi.mock("../services/ipc", () => ({
  api,
  dialogs: {},
  toIpcError: (e: unknown) =>
    typeof e === "object" && e !== null && "message" in e
      ? { kind: "invalid", message: String((e as { message: unknown }).message) }
      : { kind: "unknown", message: String(e) },
}));

const { usePluginsStore } = await import("./plugins");
const { useAppStore } = await import("./app");

const hello: PluginInfo = {
  id: "hello-plugin",
  name: "HelloPlugin",
  version: "1.0.0",
  description: "",
  author: "ChosProxy",
  dir: "/data/plugins/hello-plugin",
  command: ["python3", "hello.py"],
  requested: [{ permission: "network.read", description: "Receive traffic" }],
  granted: [],
  enabled: false,
  status: "disabled",
  message: null,
  signature: { state: "unsigned", publisher: null, key: null, fingerprint: null, problem: null },
};

describe("plugins store", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    usePluginsStore.setState({ plugins: [], logsFor: null, logs: [] });
    api.plugins.mockResolvedValue([hello]);
    api.pluginsDir.mockResolvedValue("/data/plugins");
  });

  it("lists plugins and enables one with chosen permissions", async () => {
    await usePluginsStore.getState().load();
    expect(usePluginsStore.getState().dir).toBe("/data/plugins");
    api.pluginSet.mockResolvedValue({
      ...hello,
      enabled: true,
      granted: ["network.read"],
      status: "running",
    });
    await usePluginsStore.getState().set("hello-plugin", true, ["network.read"]);
    expect(api.pluginSet).toHaveBeenCalledWith("hello-plugin", true, ["network.read"]);
    expect(usePluginsStore.getState().plugins[0]?.status).toBe("running");

    const notify = vi.spyOn(useAppStore.getState(), "notify");
    api.pluginSet.mockRejectedValue({ message: "the plugin does not ask for network.send" });
    await usePluginsStore.getState().set("hello-plugin", true, ["network.send"]);
    expect(notify).toHaveBeenCalledWith(
      "error",
      "Could not change the plugin: the plugin does not ask for network.send",
    );
  });

  it("installs the example and shows logs", async () => {
    api.pluginsInstallExample.mockResolvedValue("/data/plugins/hello-plugin");
    await usePluginsStore.getState().installExample();
    expect(api.plugins).toHaveBeenCalled();
    api.pluginLogs.mockResolvedValue([{ at: 0, level: "info", message: "Started" }]);
    await usePluginsStore.getState().showLogs("hello-plugin");
    expect(usePluginsStore.getState().logs).toHaveLength(1);
    await usePluginsStore.getState().showLogs(null);
    expect(usePluginsStore.getState().logs).toEqual([]);
  });

  it("shows the log of the plugin opened last, whatever answers first", async () => {
    let slow: (logs: { at: number; level: string; message: string }[]) => void = () => undefined;
    api.pluginLogs.mockImplementation((id: string) =>
      id === "slow"
        ? new Promise((resolve) => {
            slow = resolve;
          })
        : Promise.resolve([{ at: 1, level: "info", message: "fast" }]),
    );
    const first = usePluginsStore.getState().showLogs("slow");
    await usePluginsStore.getState().showLogs("fast");
    slow([{ at: 0, level: "info", message: "slow" }]);
    await first;
    const state = usePluginsStore.getState();
    expect(state.logsFor).toBe("fast");
    expect(state.logs.map((l) => l.message)).toEqual(["fast"]);
  });
});
