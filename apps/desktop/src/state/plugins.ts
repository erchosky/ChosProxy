// Plugins: what is installed, what each may do, and their logs.
import { create } from "zustand";
import type { PluginInfo } from "../bindings/PluginInfo";
import type { PluginLogLine } from "../bindings/PluginLogLine";
import type { PluginPermission } from "../bindings/PluginPermission";
import type { PluginsConfig } from "../bindings/PluginsConfig";
import { api, dialogs, toIpcError } from "../services/ipc";
import { useAppStore } from "./app";

interface PluginsState {
  plugins: PluginInfo[];
  dir: string;
  loaded: boolean;
  error: string | null;
  /** Plugin whose log is shown. */
  logsFor: string | null;
  logs: PluginLogLine[];

  load(): Promise<void>;
  /** Enables (approving its files) or disables a plugin with these grants. */
  set(id: string, enabled: boolean, granted: PluginPermission[]): Promise<void>;
  installExample(): Promise<void>;
  /** Trusted publishers and the signed-only switch. */
  trust: PluginsConfig | null;
  setTrusted(key: string, name: string, trust: boolean): Promise<void>;
  setRequireSigned(require: boolean): Promise<void>;
  /** Installs a plugin folder (e.g. from a ChosHub catalog). */
  installFrom(): Promise<void>;
  showLogs(id: string | null): Promise<void>;
}

export const usePluginsStore = create<PluginsState>((set, get) => ({
  plugins: [],
  dir: "",
  loaded: false,
  error: null,
  logsFor: null,
  logs: [],

  async load() {
    try {
      const [plugins, dir] = await Promise.all([api.plugins(), api.pluginsDir()]);
      set({ plugins, dir, loaded: true, error: null });
      set({ trust: await api.pluginsTrustSettings().catch(() => null) });
      const logsFor = get().logsFor;
      if (logsFor) {
        const logs = await api.pluginLogs(logsFor);
        // Another plugin's log may have been opened meanwhile.
        if (get().logsFor === logsFor) set({ logs });
      }
    } catch (e) {
      set({ loaded: true, error: toIpcError(e).message });
    }
  },

  async set(id, enabled, granted) {
    try {
      const info = await api.pluginSet(id, enabled, granted);
      set((s) => ({ plugins: s.plugins.map((p) => (p.id === id ? info : p)) }));
      useAppStore
        .getState()
        .notify("info", enabled ? `${info.name} enabled` : `${info.name} disabled`);
    } catch (e) {
      useAppStore
        .getState()
        .notify("error", `Could not change the plugin: ${toIpcError(e).message}`);
    }
  },

  trust: null,

  async setTrusted(key, name, trust) {
    try {
      await api.pluginTrust(key, name, trust);
      useAppStore
        .getState()
        .notify(
          "info",
          trust ? `${name} is now a trusted publisher` : `${name} is no longer trusted`,
        );
      await get().load();
    } catch (e) {
      useAppStore.getState().notify("error", `Could not change trust: ${toIpcError(e).message}`);
    }
  },

  async setRequireSigned(require) {
    try {
      await api.pluginsRequireSigned(require);
      await get().load();
    } catch (e) {
      useAppStore.getState().notify("error", toIpcError(e).message);
    }
  },

  async installFrom() {
    const path = await dialogs.chooseDirectory();
    if (!path) return;
    try {
      const info = await api.pluginsInstallFrom(path);
      useAppStore
        .getState()
        .notify("info", `${info.name} installed (disabled). Check its signature and permissions.`);
      await get().load();
    } catch (e) {
      useAppStore.getState().notify("error", `Not installed: ${toIpcError(e).message}`);
    }
  },

  async installExample() {
    try {
      await api.pluginsInstallExample();
      useAppStore
        .getState()
        .notify("info", "HelloPlugin installed. Review its permissions, then enable it.");
      await get().load();
    } catch (e) {
      useAppStore
        .getState()
        .notify("error", `Could not install the example: ${toIpcError(e).message}`);
    }
  },

  async showLogs(id) {
    // Which log is shown changes at once; its lines arrive later and are
    // dropped if another log was opened in between.
    set({ logsFor: id, logs: [] });
    if (!id) return;
    const logs = await api.pluginLogs(id).catch(() => []);
    if (get().logsFor === id) set({ logs });
  },
}));
