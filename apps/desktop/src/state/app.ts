import { create } from "zustand";
import type { AppInfo } from "../bindings/AppInfo";
import type { AppSettings } from "../bindings/AppSettings";
import type { BrowserInfo } from "../bindings/BrowserInfo";
import type { BrowserStatus } from "../bindings/BrowserStatus";
import type { CaStatus } from "../bindings/CaStatus";
import type { CertFormat } from "../bindings/CertFormat";
import type { DashboardStats } from "../bindings/DashboardStats";
import type { EngineEvent } from "../bindings/EngineEvent";
import type { ListenerStatus } from "../bindings/ListenerStatus";
import type { ProjectInfo } from "../bindings/ProjectInfo";
import type { ProxyConfig } from "../bindings/ProxyConfig";
import type { RecoveryStatus } from "../bindings/RecoveryStatus";
import { throttle } from "../lib/throttle";
import { api, dialogs, toIpcError } from "../services/ipc";
import { askPassword, encryptionOf, withPassword } from "./password";

export type ViewId =
  | "dashboard"
  | "intercept"
  | "history"
  | "repeater"
  | "tasks"
  | "api"
  | "fuzzer"
  | "sequencer"
  | "plugins"
  | "oast"
  | "dom"
  | "listeners"
  | "https"
  | "websockets"
  | "sitemap"
  | "scope"
  | "search"
  | "logger"
  | "project"
  | "findings"
  | "report"
  | "decoder"
  | "comparer"
  | "organizer"
  | "scripts"
  | "agent"
  | "performance"
  | "eventlog"
  | "settings";

export interface Notice {
  id: number;
  level: "info" | "warning" | "error";
  message: string;
  at: number;
}

interface AppState {
  bootstrapped: boolean;
  bootError: string | null;
  info: AppInfo | null;
  settings: AppSettings | null;
  proxyConfig: ProxyConfig | null;
  project: ProjectInfo | null;
  recovery: RecoveryStatus | null;
  listeners: ListenerStatus[];
  stats: DashboardStats | null;
  ca: CaStatus | null;
  browsers: BrowserInfo[];
  browser: BrowserStatus | null;
  notices: Notice[];
  busy: string | null;
  view: ViewId;
  paletteOpen: boolean;
  shortcutsOpen: boolean;

  bootstrap(): Promise<void>;
  handleEvent(event: EngineEvent): void;
  setView(view: ViewId): void;
  setPaletteOpen(open: boolean): void;
  setShortcutsOpen(open: boolean): void;
  notify(level: Notice["level"], message: string): void;
  dismissNotice(id: number): void;
  refreshListeners(): Promise<void>;
  refreshStats(): Promise<void>;
  newTemporaryProject(encrypt?: boolean): Promise<void>;
  createProject(): Promise<void>;
  openProject(): Promise<void>;
  openProjectReadOnly(): Promise<void>;
  restoreRecovery(): Promise<void>;
  openRecoveryReadOnly(): Promise<void>;
  discardRecovery(deleteOrphanTemp?: boolean): Promise<void>;
  dismissRecovery(): void;
  closeProject(): Promise<void>;
  /** Copies the project to a new file and continues there. */
  saveProjectAs(): Promise<void>;
  changeProjectPassword(): Promise<void>;
  startListener(id: string): Promise<void>;
  stopListener(id: string): Promise<void>;
  restartListener(id: string): Promise<void>;
  saveSettings(settings: AppSettings): Promise<boolean>;
  saveProxyConfig(config: ProxyConfig): Promise<boolean>;
  /** Re-reads settings, proxy configuration and listeners (e.g. after an import). */
  reloadConfiguration(): Promise<void>;
  /** Requires credentials on a listener, or removes them (`null`). */
  setListenerAuth(
    id: string,
    credentials: { username: string; password: string } | null,
  ): Promise<boolean>;
  refreshCa(): Promise<void>;
  exportCa(format: CertFormat): Promise<void>;
  regenerateCa(): Promise<void>;
  /** Replaces the CA with a certificate and key the user picks. */
  importCa(): Promise<void>;
  refreshBrowsers(): Promise<void>;
  launchBrowser(browserId?: string | null, url?: string | null): Promise<void>;
  closeBrowser(): Promise<void>;
}

let noticeSeq = 0;
const MAX_NOTICES = 50;

export const useAppStore = create<AppState>((set, get) => {
  // Counters are cheap to show but not to compute on large projects.
  const refreshStatsSoon = throttle(() => void get().refreshStats(), 1500);

  /** Runs an action, showing a busy label and reporting failures as notices. */
  async function run(label: string, action: () => Promise<void>): Promise<boolean> {
    set({ busy: label });
    try {
      await action();
      return true;
    } catch (e) {
      get().notify("error", `${label} failed: ${toIpcError(e).message}`);
      return false;
    } finally {
      set({ busy: null });
    }
  }

  return {
    bootstrapped: false,
    bootError: null,
    info: null,
    settings: null,
    proxyConfig: null,
    project: null,
    recovery: null,
    listeners: [],
    stats: null,
    ca: null,
    browsers: [],
    browser: null,
    notices: [],
    busy: null,
    view: "dashboard",
    paletteOpen: false,
    shortcutsOpen: false,

    async bootstrap() {
      try {
        const [info, settings, proxyConfig, project, recovery, listeners, ca, browsers, browser] =
          await Promise.all([
            api.appInfo(),
            api.getSettings(),
            api.getProxyConfig(),
            api.currentProject(),
            api.recoveryStatus(),
            api.listListeners(),
            api.caStatus(),
            api.listBrowsers(),
            api.browserStatus(),
          ]);
        set({
          info,
          settings,
          proxyConfig,
          project,
          recovery,
          listeners,
          ca,
          browsers,
          browser,
          bootstrapped: true,
        });
        for (const w of info.configWarnings) get().notify("warning", w);
        if (project) await get().refreshStats();
      } catch (e) {
        set({ bootError: toIpcError(e).message, bootstrapped: true });
      }
    },

    handleEvent(event) {
      switch (event.type) {
        case "listenerChanged":
          set((s) => ({
            listeners: s.listeners.map((l) =>
              l.config.id === event.listenerId ? { ...l, state: event.state } : l,
            ),
          }));
          if (event.state.state === "failed") {
            get().notify("error", `Listener ${event.listenerId}: ${event.state.error}`);
          }
          break;
        case "projectChanged":
          void api.currentProject().then((project) => set({ project }));
          void get().refreshListeners();
          void get().refreshStats();
          break;
        case "notice":
          get().notify(event.level, event.message);
          break;
        case "caChanged":
          void get().refreshCa();
          break;
        case "browserChanged":
          void api.browserStatus().then((browser) => set({ browser }));
          break;
        case "historyAppended":
          // History views refresh themselves; keep counters fresh here.
          refreshStatsSoon();
          break;
      }
    },

    setView: (view) => set({ view }),
    setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
    setShortcutsOpen: (shortcutsOpen) => set({ shortcutsOpen }),

    notify(level, message) {
      const notice: Notice = { id: ++noticeSeq, level, message, at: Date.now() };
      set((s) => ({ notices: [notice, ...s.notices].slice(0, MAX_NOTICES) }));
    },

    dismissNotice: (id) => set((s) => ({ notices: s.notices.filter((n) => n.id !== id) })),

    async refreshListeners() {
      try {
        set({ listeners: await api.listListeners() });
      } catch (e) {
        get().notify("error", `Could not load listeners: ${toIpcError(e).message}`);
      }
    },

    async refreshStats() {
      if (!get().project) {
        set({ stats: null });
        return;
      }
      try {
        set({ stats: await api.dashboardStats() });
      } catch (e) {
        // A project closing mid-request is not an error worth showing.
        if (toIpcError(e).kind !== "noProject") {
          get().notify("error", `Could not load statistics: ${toIpcError(e).message}`);
        }
      }
    },

    async newTemporaryProject(encrypt = false) {
      await run("Create temporary project", async () => {
        set({ project: await api.newTemporaryProject(encrypt), view: "dashboard" });
      });
    },

    async createProject() {
      const path = await dialogs.chooseProjectToCreate(get().info?.dataDir);
      if (!path) return;
      const choice = await askPassword({
        mode: "choose",
        title: "New project",
        message: "The project file can be encrypted: traffic, tokens, findings and notes.",
      });
      if (!choice) return;
      await run("Create project", async () => {
        const password = choice.encrypt ? choice.password : null;
        set({ project: await api.createProject(path, password), view: "dashboard" });
      });
    },

    async openProject() {
      const path = await dialogs.chooseProjectToOpen(get().info?.dataDir);
      if (!path) return;
      await run("Open project", async () => {
        const project = await withPassword("Open encrypted project", (pw) =>
          api.openProject(path, pw),
        );
        if (project) set({ project, view: "dashboard" });
      });
    },

    async openProjectReadOnly() {
      const path = await dialogs.chooseProjectToOpen(get().info?.dataDir);
      if (!path) return;
      await run("Open project read-only", async () => {
        const project = await withPassword("Open encrypted project (read-only)", (pw) =>
          api.openProjectReadOnly(path, pw),
        );
        if (project) {
          set({ project, view: "dashboard" });
          get().notify(
            "info",
            `Opened ${project.name} in read-only mode. Traffic capture and modifications are disabled.`,
          );
        }
      });
    },

    async restoreRecovery() {
      await run("Restore project", async () => {
        const project = await withPassword("Restore encrypted project", (pw) =>
          api.recoveryRestore(pw),
        );
        if (project) {
          set({ project, recovery: null, view: "dashboard" });
          get().notify("info", `Restored ${project.name} after unexpected exit.`);
        }
      });
    },

    async openRecoveryReadOnly() {
      await run("Open project read-only", async () => {
        const project = await withPassword("Open encrypted project (read-only)", (pw) =>
          api.recoveryOpenReadOnly(pw),
        );
        if (project) {
          set({ project, recovery: null, view: "dashboard" });
          get().notify(
            "info",
            `Opened ${project.name} in read-only mode. Traffic capture and modifications are disabled.`,
          );
        }
      });
    },

    async discardRecovery(deleteOrphanTemp = false) {
      await run("Discard recovery", async () => {
        await api.recoveryDiscard(deleteOrphanTemp);
        set({ recovery: null });
        get().notify("info", "Session recovery marker discarded.");
      });
    },

    dismissRecovery: () => set({ recovery: null }),

    async saveProjectAs() {
      const path = await dialogs.chooseProjectToCreate(get().info?.dataDir);
      if (!path) return;
      const current = get().project;
      const canKeep = current?.encrypted === true && current.kind === "persistent";
      const choice = await askPassword({
        mode: "choose",
        title: "Save project as",
        encrypt: canKeep,
        canKeep,
      });
      if (!choice) return;
      await run("Save project as", async () => {
        const project = await api.saveProjectAs(path, encryptionOf(choice, canKeep));
        set({ project });
        get().notify(
          "info",
          `Project saved to ${project.path}${project.encrypted ? " (encrypted)" : ""}`,
        );
      });
    },

    async changeProjectPassword() {
      const choice = await askPassword({ mode: "change", title: "Change the project password" });
      if (!choice) return;
      await run("Change password", async () => {
        await api.changeProjectPassword(choice.password);
        get().notify("info", "Password changed. Backups made earlier keep the old password.");
      });
    },

    async closeProject() {
      await run("Close project", async () => {
        await api.closeProject();
        set({ project: null, stats: null });
      });
    },

    async startListener(id) {
      await run(`Start listener ${id}`, async () => {
        await api.startListener(id);
      });
      await get().refreshListeners();
    },

    async stopListener(id) {
      await run(`Stop listener ${id}`, async () => {
        await api.stopListener(id);
      });
      await get().refreshListeners();
    },

    async restartListener(id) {
      await run(`Restart listener ${id}`, async () => {
        await api.restartListener(id);
      });
      await get().refreshListeners();
    },

    async saveSettings(settings) {
      return run("Save settings", async () => {
        set({ settings: await api.saveSettings(settings) });
      });
    },

    async saveProxyConfig(config) {
      return run("Save proxy configuration", async () => {
        const listeners = await api.saveProxyConfig(config);
        set({ proxyConfig: config, listeners });
      });
    },

    async setListenerAuth(id, credentials) {
      return run(credentials ? "Set proxy credentials" : "Remove proxy credentials", async () => {
        const listeners = await api.setListenerAuth(
          id,
          credentials?.username ?? null,
          credentials?.password ?? null,
        );
        set({ listeners, proxyConfig: await api.getProxyConfig() });
        get().notify(
          "info",
          credentials
            ? `Listener ${id} now requires a user name and password`
            : `Listener ${id} no longer requires credentials`,
        );
      });
    },

    async reloadConfiguration() {
      try {
        const [settings, proxyConfig, listeners] = await Promise.all([
          api.getSettings(),
          api.getProxyConfig(),
          api.listListeners(),
        ]);
        set({ settings, proxyConfig, listeners });
      } catch (e) {
        get().notify("error", `Could not reload the configuration: ${toIpcError(e).message}`);
      }
    },

    async refreshCa() {
      try {
        set({ ca: await api.caStatus() });
      } catch (e) {
        get().notify("error", `Could not read the CA status: ${toIpcError(e).message}`);
      }
    },

    async exportCa(format) {
      const path = await dialogs.chooseCertificateDestination(format);
      if (!path) return;
      await run("Export CA certificate", async () => {
        const written = await api.exportCa(path, format);
        get().notify("info", `CA certificate saved to ${written}`);
      });
    },

    async importCa() {
      const cert = await dialogs.choosePemFile("CA certificate (PEM)");
      if (!cert) return;
      const key = await dialogs.choosePemFile("Its private key (PEM, PKCS#8, unencrypted)");
      if (!key) return;
      const ok = await dialogs.confirm(
        "The imported certificate authority will sign every intercepted HTTPS certificate. " +
          "Browsers must trust it. The current CA is kept as ca-cert.previous.pem.",
        "Import this CA?",
        "Import",
      );
      if (!ok) return;
      await run("Import CA", async () => {
        set({ ca: await api.importCa(cert, key) });
      });
    },

    async regenerateCa() {
      const ok = await dialogs.confirm(
        "A new certificate authority will replace the current one. Browsers that trust the " +
          "current CA will show certificate errors until you import the new certificate. " +
          "The old private key is deleted.",
        "Regenerate local CA?",
        "Regenerate",
      );
      if (!ok) return;
      await run("Regenerate CA", async () => {
        set({ ca: await api.regenerateCa() });
      });
    },

    async refreshBrowsers() {
      try {
        const [browsers, browser] = await Promise.all([api.listBrowsers(), api.browserStatus()]);
        set({ browsers, browser });
      } catch (e) {
        get().notify("error", `Could not list browsers: ${toIpcError(e).message}`);
      }
    },

    async launchBrowser(browserId = null, url = null) {
      await run("Open lab browser", async () => {
        set({ browser: await api.launchBrowser(browserId, url) });
      });
    },

    async closeBrowser() {
      await run("Close lab browser", async () => {
        await api.closeBrowser();
        set({ browser: await api.browserStatus() });
      });
    },
  };
});
