import { lazy, Suspense, useEffect } from "react";
import { Icon } from "../components/Icon";
import { LoadingState } from "../components/States";
import { DashboardView } from "../features/dashboard/DashboardView";
import { ContextMenu } from "../components/ContextMenu";
import { CrawlDialog } from "../features/tasks/CrawlDialog";
import { DiscoveryDialog } from "../features/tasks/DiscoveryDialog";
import { useEngageStore } from "../state/engage";
import { ScriptActionDialog } from "../features/scripts/ScriptActionDialog";
import { useScriptsStore } from "../state/scripts";
import { AiDialog } from "../features/ai/AiDialog";
import { AddToFindingDialog } from "../features/findings/AddToFindingDialog";
import { useAppStore } from "../state/app";
import { NAV } from "./nav";
import { viewShortcut } from "./shortcuts";
import { useAgentStore } from "../state/agent";
import { useInterceptStore } from "../state/intercept";
import { StatusBar } from "./StatusBar";
import { TopBar } from "./TopBar";

// Less used views load on first use, keeping startup small.
// The engagement dialogs show messages in the code editor, which is the
// largest part of the bundle: they are loaded when one is opened, not with
// the shell (the startup script had grown from 265 kB to 866 kB).
const EngageDialogs = lazy(() =>
  import("../features/engage/EngageDialogs").then((m) => ({ default: m.EngageDialogs })),
);
const HistoryView = lazy(() =>
  import("../features/proxy/HistoryView").then((m) => ({ default: m.HistoryView })),
);
const RepeaterView = lazy(() =>
  import("../features/repeater/RepeaterView").then((m) => ({ default: m.RepeaterView })),
);
const PluginsView = lazy(() =>
  import("../features/plugins/PluginsView").then((m) => ({ default: m.PluginsView })),
);
const ScriptsView = lazy(() =>
  import("../features/scripts/ScriptsView").then((m) => ({ default: m.ScriptsView })),
);
const DomInspectorView = lazy(() =>
  import("../features/dom/DomInspectorView").then((m) => ({ default: m.DomInspectorView })),
);
const OastView = lazy(() =>
  import("../features/oast/OastView").then((m) => ({ default: m.OastView })),
);
const SequencerView = lazy(() =>
  import("../features/sequencer/SequencerView").then((m) => ({ default: m.SequencerView })),
);
const FuzzerView = lazy(() =>
  import("../features/fuzzer/FuzzerView").then((m) => ({ default: m.FuzzerView })),
);
const ApiView = lazy(() => import("../features/api/ApiView").then((m) => ({ default: m.ApiView })));
const TasksView = lazy(() =>
  import("../features/tasks/TasksView").then((m) => ({ default: m.TasksView })),
);
const InterceptView = lazy(() =>
  import("../features/intercept/InterceptView").then((m) => ({ default: m.InterceptView })),
);
const ProjectView = lazy(() =>
  import("../features/project/ProjectView").then((m) => ({ default: m.ProjectView })),
);
const HttpsView = lazy(() =>
  import("../features/https/HttpsView").then((m) => ({ default: m.HttpsView })),
);
const ListenersView = lazy(() =>
  import("../features/proxy/ListenersView").then((m) => ({ default: m.ListenersView })),
);
const WebSocketsView = lazy(() =>
  import("../features/websockets/WebSocketsView").then((m) => ({ default: m.WebSocketsView })),
);
const SiteMapView = lazy(() =>
  import("../features/target/SiteMapView").then((m) => ({ default: m.SiteMapView })),
);
const ScopeView = lazy(() =>
  import("../features/target/ScopeView").then((m) => ({ default: m.ScopeView })),
);
const SearchView = lazy(() =>
  import("../features/search/SearchView").then((m) => ({ default: m.SearchView })),
);
const FindingsView = lazy(() =>
  import("../features/findings/FindingsView").then((m) => ({ default: m.FindingsView })),
);
const ReportView = lazy(() =>
  import("../features/report/ReportView").then((m) => ({ default: m.ReportView })),
);
const LoggerView = lazy(() =>
  import("../features/logger/LoggerView").then((m) => ({ default: m.LoggerView })),
);
const DecoderView = lazy(() =>
  import("../features/decoder/DecoderView").then((m) => ({ default: m.DecoderView })),
);
const ComparerView = lazy(() =>
  import("../features/comparer/ComparerView").then((m) => ({ default: m.ComparerView })),
);
const OrganizerView = lazy(() =>
  import("../features/organizer/OrganizerView").then((m) => ({ default: m.OrganizerView })),
);
const PerformanceView = lazy(() =>
  import("../features/performance/PerformanceView").then((m) => ({
    default: m.PerformanceView,
  })),
);
const AgentView = lazy(() =>
  import("../features/agent/AgentView").then((m) => ({ default: m.AgentView })),
);
const EventLogView = lazy(() =>
  import("../features/eventlog/EventLogView").then((m) => ({ default: m.EventLogView })),
);
const SettingsView = lazy(() =>
  import("../features/settings/SettingsView").then((m) => ({ default: m.SettingsView })),
);

const NO_BINDINGS: never[] = [];

export function Shell() {
  const view = useAppStore((s) => s.view);
  const setView = useAppStore((s) => s.setView);
  const project = useAppStore((s) => s.project);
  const saveProjectAs = useAppStore((s) => s.saveProjectAs);
  const waiting = useInterceptStore((s) => s.state.pending.length);
  const agentWaiting = useAgentStore((s) => s.approvals.length);
  const engageOpen = useEngageStore(
    (s) => s.csrfFor !== null || s.frameFor !== null || s.analyzeFor !== null,
  );
  const bindings = useAppStore((s) => s.settings?.ui.keyBindings) ?? NO_BINDINGS;
  // Request menus list action scripts, so the library is loaded up front.
  useEffect(() => {
    void useScriptsStore.getState().load();
  }, []);
  return (
    <div className="shell">
      <TopBar />
      <nav className="nav" aria-label="Main">
        {NAV.map((item, i) => (
          <div key={item.id}>
            {item.group && NAV[i - 1]?.group !== item.group && (
              <div className="nav-group">{item.group}</div>
            )}
            <button
              type="button"
              className="nav-item"
              aria-current={view === item.id ? "page" : undefined}
              title={
                viewShortcut(item.id, bindings)
                  ? `${item.label} (${viewShortcut(item.id, bindings)})`
                  : undefined
              }
              onClick={() => setView(item.id)}
            >
              <Icon name={item.icon} />
              <span>{item.label}</span>
              {item.id === "intercept" && waiting > 0 && (
                <span className="nav-count" aria-label={`${waiting} waiting`}>
                  {waiting}
                </span>
              )}
              {item.id === "agent" && agentWaiting > 0 && (
                <span className="nav-count" aria-label={`${agentWaiting} waiting for approval`}>
                  {agentWaiting}
                </span>
              )}
            </button>
          </div>
        ))}
      </nav>
      <main className="workspace">
        {project?.readOnly && (
          <div className="callout callout-warn" role="status" style={{ margin: "8px 16px 0" }}>
            <Icon name="shield" />
            <span>
              <strong>Read-only mode:</strong> this project is open for inspection only. Proxy
              listeners, active tools, and database modifications are disabled.
            </span>
            <span className="spacer" />
            <button type="button" className="btn btn-sm" onClick={() => void saveProjectAs()}>
              Save as writable project…
            </button>
          </div>
        )}
        <Suspense fallback={<LoadingState />}>
          {view === "dashboard" && <DashboardView />}
          {view === "project" && <ProjectView />}
          {view === "intercept" && <InterceptView />}
          {view === "history" && <HistoryView />}
          {view === "repeater" && <RepeaterView />}
          {view === "tasks" && <TasksView />}
          {view === "api" && <ApiView />}
          {view === "fuzzer" && <FuzzerView />}
          {view === "sequencer" && <SequencerView />}
          {view === "oast" && <OastView />}
          {view === "dom" && <DomInspectorView />}
          {view === "scripts" && <ScriptsView />}
          {view === "plugins" && <PluginsView />}
          {view === "listeners" && <ListenersView />}
          {view === "https" && <HttpsView />}
          {view === "websockets" && <WebSocketsView />}
          {view === "sitemap" && <SiteMapView />}
          {view === "scope" && <ScopeView />}
          {view === "search" && <SearchView />}
          {view === "findings" && <FindingsView />}
          {view === "report" && <ReportView />}
          {view === "logger" && <LoggerView />}
          {view === "decoder" && <DecoderView />}
          {view === "comparer" && <ComparerView />}
          {view === "organizer" && <OrganizerView />}
          {view === "agent" && <AgentView />}
          {view === "performance" && <PerformanceView />}
          {view === "eventlog" && <EventLogView />}
          {view === "settings" && <SettingsView />}
        </Suspense>
      </main>
      <StatusBar />
      <AddToFindingDialog />
      <AiDialog />
      <ContextMenu />
      <CrawlDialog />
      <DiscoveryDialog />
      {engageOpen && (
        <Suspense fallback={null}>
          <EngageDialogs />
        </Suspense>
      )}
      <ScriptActionDialog />
    </div>
  );
}
