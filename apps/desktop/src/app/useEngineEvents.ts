import { useEffect } from "react";
import { onEngineEvent } from "../services/ipc";
import { useScriptsStore } from "../state/scripts";
import { useAppStore } from "../state/app";
import { useEventLogStore } from "../state/eventLog";
import { throttle } from "../lib/throttle";
import { allHistoryStores, useHistoryStore, useLoggerStore } from "../state/history";
import { useInterceptStore } from "../state/intercept";
import { useFindingsStore } from "../state/findings";
import { useRepeaterStore } from "../state/repeater";
import { useTasksStore } from "../state/tasks";
import { useFuzzerStore } from "../state/fuzzer";
import { useSequencerStore } from "../state/sequencer";
import { useDomStore } from "../state/dom";
import { useOastStore } from "../state/oast";
import { usePluginsStore } from "../state/plugins";
import { useReportStore } from "../state/report";
import { useOrganizerStore } from "../state/organizer";
import { useProjectStore } from "../state/project";
import { useScopeStore } from "../state/scope";
import { useAgentStore } from "../state/agent";
import { useSearchStore } from "../state/search";
import { useSiteMapStore } from "../state/siteMap";
import { useWebSocketsStore } from "../state/websockets";

/** Refreshes the traffic list on screen (other lists reload when opened). */
const refreshVisibleTraffic = throttle(() => {
  const view = useAppStore.getState().view;
  if (view === "history") useHistoryStore.getState().invalidate();
  if (view === "logger") useLoggerStore.getState().invalidate();
}, 300);

/** New automatic findings: refresh the counts and an open Findings list. */
const refreshFindings = throttle(() => {
  void useAppStore.getState().refreshStats();
  const findings = useFindingsStore.getState();
  if (useAppStore.getState().view === "findings") {
    void findings.load();
    void findings.loadPassive();
  }
}, 2000);

/** Task changes: refresh the Tasks view or the fuzzer results while open. */
const refreshTasks = throttle(() => {
  const view = useAppStore.getState().view;
  if (view === "tasks") void useTasksStore.getState().refresh();
  if (view === "sequencer") void useSequencerStore.getState().loadCaptures();
  if (view === "fuzzer" && useFuzzerStore.getState().tab === "results") {
    const fuzzer = useFuzzerStore.getState();
    void fuzzer.loadAttacks().then(() => fuzzer.loadResults());
  }
}, 700);

/** Plugin changes: refresh the Plugins view while it is open. */
const refreshPlugins = throttle(() => {
  if (useAppStore.getState().view === "plugins") void usePluginsStore.getState().load();
}, 700);

/** DOM Inspector changes: refresh its view while it is open. */
const refreshDom = throttle(() => {
  if (useAppStore.getState().view === "dom") void useDomStore.getState().load();
}, 700);

/** OAST changes: refresh the OAST view while it is open. */
const refreshOast = throttle(() => {
  if (useAppStore.getState().view === "oast") void useOastStore.getState().load();
}, 700);

/** Agent activity: refresh the approvals badge and the open task. */
const refreshAgent = throttle(() => {
  const agent = useAgentStore.getState();
  if (useAppStore.getState().view === "agent") void agent.load();
  else void agent.refresh();
}, 300);

/** Routes engine events to the stores for the lifetime of the app. */
export function useEngineEvents() {
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    void onEngineEvent((event) => {
      useAppStore.getState().handleEvent(event);
      useEventLogStore.getState().onEvent(event);
      if (event.type === "webSocketsChanged") useWebSocketsStore.getState().onChanged();
      if (event.type === "historyAppended") refreshVisibleTraffic();
      if (event.type === "findingsChanged") refreshFindings();
      if (event.type === "pluginsChanged") {
        refreshPlugins();
        void useScriptsStore.getState().loadPluginActions();
      }
      if (event.type === "oastChanged") refreshOast();
      if (event.type === "domChanged") refreshDom();
      if (event.type === "agentChanged") refreshAgent();
      if (event.type === "tasksChanged") {
        refreshTasks();
        if (useAppStore.getState().view === "agent") refreshAgent();
        refreshVisibleTraffic();
      }
      if (event.type === "projectChanged") {
        for (const store of allHistoryStores) store.getState().reset();
        useSiteMapStore.getState().reset();
        useSearchStore.getState().reset();
        useOrganizerStore.getState().reset();
        useWebSocketsStore.getState().reset();
        useProjectStore.getState().reset();
        useFindingsStore.getState().reset();
        useReportStore.getState().reset();
        useRepeaterStore.getState().reset();
        useTasksStore.getState().reset();
        useAgentStore.getState().reset();
        useFuzzerStore.getState().reset();
        useSequencerStore.getState().reset();
        void useScopeStore.getState().load();
      }
      if (event.type === "scopeChanged") {
        // In-scope columns and scope filters depend on the rules.
        void useScopeStore.getState().load();
        refreshVisibleTraffic();
        if (useSiteMapStore.getState().map) void useSiteMapStore.getState().load();
      }
      if (event.type === "interceptChanged" || event.type === "projectChanged") {
        void useInterceptStore.getState().refresh();
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
}
