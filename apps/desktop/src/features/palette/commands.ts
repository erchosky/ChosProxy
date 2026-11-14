import type { ListenerStatus } from "../../bindings/ListenerStatus";
import type { ProjectInfo } from "../../bindings/ProjectInfo";
import type { Theme } from "../../bindings/Theme";
import { useAiStore } from "../../state/ai";
import type { ViewId } from "../../state/app";

export interface Command {
  id: string;
  title: string;
  group: string;
  run(): void;
}

export interface CommandContext {
  project: ProjectInfo | null;
  listeners: ListenerStatus[];
  theme: Theme;
  setView(view: ViewId): void;
  newTemporaryProject(): void;
  newEncryptedTemporaryProject(): void;
  createProject(): void;
  openProject(): void;
  closeProject(): void;
  saveProjectAs(): void;
  newCrawl(): void;
  newOastPayload(): void;
  startListener(id: string): void;
  stopListener(id: string): void;
  setTheme(theme: Theme): void;
  browserRunning: boolean;
  openBrowser(): void;
  closeBrowser(): void;
  caAvailable: boolean;
  exportCa(): void;
  interceptEnabled: boolean;
  setIntercept(enabled: boolean): void;
  showShortcuts(): void;
}

/** Commands available in the current state. Only real, working actions. */
export function buildCommands(ctx: CommandContext): Command[] {
  const cmds: Command[] = [];
  const nextTheme: Record<Theme, Theme> = { dark: "light", light: "system", system: "dark" };
  if (ctx.project) {
    cmds.push(
      {
        id: "go-dashboard",
        title: "Go to Dashboard",
        group: "Navigate",
        run: () => ctx.setView("dashboard"),
      },
      {
        id: "go-intercept",
        title: "Go to Intercept",
        group: "Navigate",
        run: () => ctx.setView("intercept"),
      },
      {
        id: "go-history",
        title: "Go to HTTP history",
        group: "Navigate",
        run: () => ctx.setView("history"),
      },
      {
        id: "go-listeners",
        title: "Go to Proxy listeners",
        group: "Navigate",
        run: () => ctx.setView("listeners"),
      },
      {
        id: "go-https",
        title: "Go to HTTPS & lab browser",
        group: "Navigate",
        run: () => ctx.setView("https"),
      },
      {
        id: "go-project",
        title: "Project: backups, notes, audit log",
        group: "Navigate",
        run: () => ctx.setView("project"),
      },
      {
        id: "save-project-as",
        title: "Save project as…",
        group: "Project",
        run: ctx.saveProjectAs,
      },
      {
        id: "go-findings",
        title: "Go to Findings",
        group: "Navigate",
        run: () => ctx.setView("findings"),
      },
      {
        id: "go-report",
        title: "Go to Report",
        group: "Navigate",
        run: () => ctx.setView("report"),
      },
      {
        id: "go-websockets",
        title: "Go to WebSockets history",
        group: "Navigate",
        run: () => ctx.setView("websockets"),
      },
      {
        id: "go-sitemap",
        title: "Go to Site map",
        group: "Navigate",
        run: () => ctx.setView("sitemap"),
      },
      {
        id: "go-scope",
        title: "Edit scope",
        group: "Navigate",
        run: () => ctx.setView("scope"),
      },
      {
        id: "go-search",
        title: "Search all traffic",
        group: "Navigate",
        run: () => ctx.setView("search"),
      },
      {
        id: "go-logger",
        title: "Go to Logger (all tools' traffic)",
        group: "Navigate",
        run: () => ctx.setView("logger"),
      },
      {
        id: "new-crawl",
        title: "New crawl…",
        group: "Navigate",
        run: ctx.newCrawl,
      },
      {
        id: "new-oast",
        title: "New OAST payload (copy its URL)",
        group: "Tools",
        run: ctx.newOastPayload,
      },
      {
        id: "go-tasks",
        title: "Open Crawler & tasks",
        group: "Navigate",
        run: () => ctx.setView("tasks"),
      },
      {
        id: "go-plugins",
        title: "Open Plugins",
        group: "Navigate",
        run: () => ctx.setView("plugins"),
      },
      {
        id: "go-sequencer",
        title: "Open Sequencer",
        group: "Navigate",
        run: () => ctx.setView("sequencer"),
      },
      {
        id: "go-fuzzer",
        title: "Open Fuzzer",
        group: "Navigate",
        run: () => ctx.setView("fuzzer"),
      },
      {
        id: "go-repeater",
        title: "Open Repeater",
        group: "Navigate",
        run: () => ctx.setView("repeater"),
      },
      {
        id: "go-decoder",
        title: "Open Decoder",
        group: "Navigate",
        run: () => ctx.setView("decoder"),
      },
      {
        id: "go-comparer",
        title: "Open Comparer",
        group: "Navigate",
        run: () => ctx.setView("comparer"),
      },
      {
        id: "go-organizer",
        title: "Open Organizer",
        group: "Navigate",
        run: () => ctx.setView("organizer"),
      },
      {
        id: "go-performance",
        title: "Open Performance",
        group: "Navigate",
        run: () => ctx.setView("performance"),
      },
      {
        id: "go-agent",
        title: "Open ChosAgent",
        group: "Navigate",
        run: () => ctx.setView("agent"),
      },
      {
        id: "ai-record-login",
        title: "Record a login for ChosAI",
        group: "Tools",
        run: () =>
          useAiStore.getState().open({ title: "Login flow", attachments: [], task: "loginFlow" }),
      },
      {
        id: "go-eventlog",
        title: "Open event log",
        group: "Navigate",
        run: () => ctx.setView("eventlog"),
      },
      {
        id: "go-settings",
        title: "Open Settings",
        group: "Navigate",
        run: () => ctx.setView("settings"),
      },
      {
        id: "go-security-center",
        title: "Go to Security Center",
        group: "Navigate",
        run: () => ctx.setView("settings"),
      },
    );
    for (const l of ctx.listeners) {
      const running = l.state.state === "running";
      cmds.push({
        id: `listener-${l.config.id}`,
        title: `${running ? "Stop" : "Start"} proxy listener ${l.config.id} (${l.config.bind_address}:${l.config.port})`,
        group: "Proxy",
        run: () => (running ? ctx.stopListener(l.config.id) : ctx.startListener(l.config.id)),
      });
    }
    cmds.push({
      id: "toggle-intercept",
      title: ctx.interceptEnabled ? "Turn Intercept off" : "Turn Intercept on",
      group: "Proxy",
      run: () => ctx.setIntercept(!ctx.interceptEnabled),
    });
    cmds.push(
      ctx.browserRunning
        ? { id: "browser", title: "Close lab browser", group: "Browser", run: ctx.closeBrowser }
        : { id: "browser", title: "Open lab browser", group: "Browser", run: ctx.openBrowser },
    );
    if (ctx.caAvailable) {
      cmds.push({
        id: "export-ca",
        title: "Export CA certificate (PEM)…",
        group: "HTTPS",
        run: ctx.exportCa,
      });
    }
    cmds.push({
      id: "close-project",
      title: "Close project",
      group: "Project",
      run: ctx.closeProject,
    });
  }
  cmds.push(
    {
      id: "new-temp",
      title: "New temporary project",
      group: "Project",
      run: ctx.newTemporaryProject,
    },
    {
      id: "new-temp-encrypted",
      title: "New encrypted temporary project",
      group: "Project",
      run: ctx.newEncryptedTemporaryProject,
    },
    { id: "new-file", title: "New project file…", group: "Project", run: ctx.createProject },
    { id: "open-file", title: "Open project file…", group: "Project", run: ctx.openProject },
    {
      id: "theme",
      title: `Switch theme to ${nextTheme[ctx.theme]}`,
      group: "Appearance",
      run: () => ctx.setTheme(nextTheme[ctx.theme]),
    },
    { id: "shortcuts", title: "Keyboard shortcuts", group: "Help", run: ctx.showShortcuts },
  );
  return cmds;
}
