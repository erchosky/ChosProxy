import { create } from "zustand";
import type { ScopeConfig } from "../bindings/ScopeConfig";
import type { ScopeDecision } from "../bindings/ScopeDecision";
import { api, toIpcError } from "../services/ipc";
import { useAppStore } from "./app";

interface ScopeState {
  config: ScopeConfig | null;
  /** The open project uses its own scope. */
  projectOverride: boolean;
  error: string | null;
  saving: boolean;

  load(): Promise<void>;
  /** Switches between the project's own scope and the global one. */
  setOverride(enabled: boolean): Promise<void>;
  /** Saves rules; resolves to an error message, or null on success. */
  save(config: ScopeConfig): Promise<string | null>;
  check(url: string): Promise<ScopeDecision>;
  /** Adds the URL's origin (or origin + path) to the include or exclude list. */
  addUrl(url: string, exclude: boolean, path: boolean): Promise<void>;
}

/** Enabled include rules (nothing is in scope without one). */
export function activeIncludes(config: ScopeConfig | null): number {
  return config?.include.filter((r) => r.enabled).length ?? 0;
}

export const useScopeStore = create<ScopeState>((set, get) => ({
  config: null,
  projectOverride: false,
  error: null,
  saving: false,

  async load() {
    try {
      const [config, projectOverride] = await Promise.all([
        api.getScope(),
        api.scopeOverride().catch(() => false),
      ]);
      set({ config, projectOverride, error: null });
    } catch (e) {
      set({ error: toIpcError(e).message });
    }
  },

  async save(config) {
    set({ saving: true });
    try {
      set({ config: await api.saveScope(config), error: null });
      return null;
    } catch (e) {
      return toIpcError(e).message;
    } finally {
      set({ saving: false });
    }
  },

  check: (url) => api.checkScope(url),

  async setOverride(enabled) {
    try {
      await api.setScopeOverride(enabled);
      await get().load();
    } catch (e) {
      useAppStore
        .getState()
        .notify("error", `Could not change the scope source: ${toIpcError(e).message}`);
    }
  },

  async addUrl(url, exclude, path) {
    try {
      set({ config: await api.addUrlToScope(url, exclude, path) });
      useAppStore
        .getState()
        .notify("info", `${exclude ? "Excluded from" : "Added to"} scope: ${url}`);
    } catch (e) {
      useAppStore.getState().notify("error", `Scope change failed: ${toIpcError(e).message}`);
    }
  },
}));
