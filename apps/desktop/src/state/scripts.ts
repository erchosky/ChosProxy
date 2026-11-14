import { create } from "zustand";
import type { PluginAction } from "../bindings/PluginAction";
import type { ScriptEntry } from "../bindings/ScriptEntry";
import type { ScriptKind } from "../bindings/ScriptKind";
import type { ScriptLibrary } from "../bindings/ScriptLibrary";
import type { ScriptRun } from "../bindings/ScriptRun";
import type { ScriptsConfig } from "../bindings/ScriptsConfig";
import { api, dialogs, toIpcError } from "../services/ipc";
import { useAppStore } from "./app";

/** An action script's result, shown in a dialog. */
export interface ActionResult {
  name: string;
  exchangeId: number;
  run: ScriptRun | null;
  error: string | null;
}

interface ScriptsState {
  library: ScriptLibrary | null;
  error: string | null;
  action: ActionResult | null;
  /** Runs an action script on a stored exchange and shows the result. */
  runAction(id: string, name: string, exchangeId: number): Promise<void>;
  closeAction(): void;
  /** Actions running plugins added to request menus. */
  pluginActions: PluginAction[];
  loadPluginActions(): Promise<void>;
  /** Runs a plugin action on a stored exchange and shows its output. */
  runPluginAction(a: PluginAction, exchangeId: number): Promise<void>;
  load(): Promise<void>;
  /** Saves the library; returns false (with a notice) on failure. */
  save(config: ScriptsConfig): Promise<boolean>;
  /** Writes scripts (all when `ids` is empty) to a JSON file the user picks. */
  exportScripts(ids: string[]): Promise<void>;
  /** Adds the scripts of a JSON file (switched off). */
  importScripts(): Promise<void>;
}

/** The ChosScripts library (Tools → Scripts). */
export const useScriptsStore = create<ScriptsState>((set) => ({
  library: null,
  error: null,
  action: null,

  async runAction(id, name, exchangeId) {
    set({ action: { name, exchangeId, run: null, error: null } });
    try {
      const run = await api.scriptAction(id, exchangeId);
      set({ action: { name, exchangeId, run, error: null } });
    } catch (e) {
      set({ action: { name, exchangeId, run: null, error: toIpcError(e).message } });
    }
  },

  closeAction: () => set({ action: null }),

  pluginActions: [],
  async loadPluginActions() {
    try {
      set({ pluginActions: await api.pluginActions() });
    } catch {
      set({ pluginActions: [] });
    }
  },

  async runPluginAction(a, exchangeId) {
    const name = `${a.pluginName}: ${a.label}`;
    set({ action: { name, exchangeId, run: null, error: null } });
    const started = Date.now();
    try {
      const output = await api.pluginRunAction(a.plugin, a.action, exchangeId);
      set({
        action: {
          name,
          exchangeId,
          run: {
            problem: null,
            output,
            findings: [],
            printed: [],
            elapsedMs: Date.now() - started,
          },
          error: null,
        },
      });
    } catch (e) {
      set({ action: { name, exchangeId, run: null, error: toIpcError(e).message } });
    }
  },

  async load() {
    try {
      set({ library: await api.scripts(), error: null });
    } catch (e) {
      set({ error: toIpcError(e).message });
    }
  },

  async exportScripts(ids) {
    const notify = useAppStore.getState().notify;
    const path = await dialogs.chooseJsonDestination("chosproxy-scripts.json");
    if (!path) return;
    try {
      const n = await api.exportScripts(path, ids);
      notify("info", `${n} script${n === 1 ? "" : "s"} written to ${path}`);
    } catch (e) {
      notify("error", `Scripts not exported: ${toIpcError(e).message}`);
    }
  },

  async importScripts() {
    const notify = useAppStore.getState().notify;
    const path = await dialogs.chooseJsonFile();
    if (!path) return;
    try {
      const before = new Set(useScriptsStore.getState().library?.config.scripts.map((s) => s.id));
      const library = await api.importScripts(path);
      set({ library, error: null });
      const added = library.config.scripts.filter((s) => !before.has(s.id)).length;
      notify(
        "info",
        `${added} script${added === 1 ? "" : "s"} imported, switched off: read them, then enable them.`,
      );
    } catch (e) {
      notify("error", `Scripts not imported: ${toIpcError(e).message}`);
    }
  },

  async save(config) {
    try {
      set({ library: await api.saveScripts(config), error: null });
      return true;
    } catch (e) {
      useAppStore.getState().notify("error", `Scripts not saved: ${toIpcError(e).message}`);
      return false;
    }
  },
}));

/** Enabled scripts of `kind` that compile, for menus. */
export function usableScripts(library: ScriptLibrary | null, kind: ScriptKind): ScriptEntry[] {
  if (!library) return [];
  return library.config.scripts.filter(
    (s) => s.enabled && s.kind === kind && !(s.id in library.problems),
  );
}
