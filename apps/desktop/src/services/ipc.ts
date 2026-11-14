// The only module that talks to Tauri. Everything else imports from here.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { confirm, open, save } from "@tauri-apps/plugin-dialog";
import type { ActiveCheck } from "../bindings/ActiveCheck";
import type { PointKind } from "../bindings/PointKind";
import type { AgentApproval } from "../bindings/AgentApproval";
import type { AgentConfig } from "../bindings/AgentConfig";
import type { AgentStart } from "../bindings/AgentStart";
import type { AgentState } from "../bindings/AgentState";
import type { AgentToolInfo } from "../bindings/AgentToolInfo";
import type { AiActivity } from "../bindings/AiActivity";
import type { AiAnswer } from "../bindings/AiAnswer";
import type { PerformanceSnapshot } from "../bindings/PerformanceSnapshot";
import type { TempCleanup } from "../bindings/TempCleanup";
import type { TempStatus } from "../bindings/TempStatus";
import type { AiConfig } from "../bindings/AiConfig";
import type { AiPreview } from "../bindings/AiPreview";
import type { AiRequest } from "../bindings/AiRequest";
import type { LoginRecording } from "../bindings/LoginRecording";
import type { ApiConfig } from "../bindings/ApiConfig";
import type { ApiDefinitionDetail } from "../bindings/ApiDefinitionDetail";
import type { ApiDefinitionInfo } from "../bindings/ApiDefinitionInfo";
import type { ApiRequest } from "../bindings/ApiRequest";
import type { ApiSendConfig } from "../bindings/ApiSendConfig";
import type { ApiStatus } from "../bindings/ApiStatus";
import type { GraphqlOperation } from "../bindings/GraphqlOperation";
import type { GraphqlSchema } from "../bindings/GraphqlSchema";
import type { NewSuppression } from "../bindings/NewSuppression";
import type { SiteNodeDetails } from "../bindings/SiteNodeDetails";
import type { ScanConfig } from "../bindings/ScanConfig";
import type { ScanSummary } from "../bindings/ScanSummary";
import type { LiveTaskConfig } from "../bindings/LiveTaskConfig";
import type { PoolsConfig } from "../bindings/PoolsConfig";
import type { PoolsView } from "../bindings/PoolsView";
import type { SuppressionRule } from "../bindings/SuppressionRule";
import type { ScanLaunch } from "../bindings/ScanLaunch";
import type { AppInfo } from "../bindings/AppInfo";
import type { DiscoveryConfig } from "../bindings/DiscoveryConfig";
import type { DiscoveryHit } from "../bindings/DiscoveryHit";
import type { DomEventRow } from "../bindings/DomEventRow";
import type { TargetAnalysis } from "../bindings/TargetAnalysis";
import type { ScriptedSiteMap } from "../bindings/ScriptedSiteMap";
import type { SiteFilter } from "../bindings/SiteFilter";
import type { ToolConfig } from "../bindings/ToolConfig";
import type { SiteDiffEntry } from "../bindings/SiteDiffEntry";
import type { SiteSnapshotInfo } from "../bindings/SiteSnapshotInfo";
import type { NewSchedule } from "../bindings/NewSchedule";
import type { ScheduleRow } from "../bindings/ScheduleRow";
import type { DomStatus } from "../bindings/DomStatus";
import type { BrowserCrawlConfig } from "../bindings/BrowserCrawlConfig";
import type { CrawlGraph } from "../bindings/CrawlGraph";
import type { CredentialInfo } from "../bindings/CredentialInfo";
import type { KeyringProtection } from "../bindings/KeyringProtection";
import type { KeyringStatus } from "../bindings/KeyringStatus";
import type { SessionRule } from "../bindings/SessionRule";
import type { NetworkConfig } from "../bindings/NetworkConfig";
import type { ScriptCell } from "../bindings/ScriptCell";
import type { ScriptKind } from "../bindings/ScriptKind";
import type { ScriptLibrary } from "../bindings/ScriptLibrary";
import type { ScriptRun } from "../bindings/ScriptRun";
import type { ScriptedHistoryPage } from "../bindings/ScriptedHistoryPage";
import type { ScriptsConfig } from "../bindings/ScriptsConfig";
import type { OastConfig } from "../bindings/OastConfig";
import type { OastInteractionDetail } from "../bindings/OastInteractionDetail";
import type { OastInteractionRow } from "../bindings/OastInteractionRow";
import type { OastPayloadRow } from "../bindings/OastPayloadRow";
import type { OastSelfHostedConfig } from "../bindings/OastSelfHostedConfig";
import type { OastStatus } from "../bindings/OastStatus";
import type { AppSettings } from "../bindings/AppSettings";
import type { AuditEntry } from "../bindings/AuditEntry";
import type { AuditQuery } from "../bindings/AuditQuery";
import type { BackupInfo } from "../bindings/BackupInfo";
import type { SecurityOverview } from "../bindings/SecurityOverview";
import type { BackupSettings } from "../bindings/BackupSettings";
import type { ProjectFileInfo } from "../bindings/ProjectFileInfo";
import type { ProjectSettings } from "../bindings/ProjectSettings";
import type { SettingsImportPreview } from "../bindings/SettingsImportPreview";
import type { SettingsSection } from "../bindings/SettingsSection";
import type { BrowserInfo } from "../bindings/BrowserInfo";
import type { BrowserStatus } from "../bindings/BrowserStatus";
import type { CaStatus } from "../bindings/CaStatus";
import type { CertFormat } from "../bindings/CertFormat";
import type { DecodeStep } from "../bindings/DecodeStep";
import type { FindingDetail } from "../bindings/FindingDetail";
import type { FindingQuery } from "../bindings/FindingQuery";
import type { FindingRow } from "../bindings/FindingRow";
import type { FindingUpdate } from "../bindings/FindingUpdate";
import type { NewFinding } from "../bindings/NewFinding";
import type { Operation } from "../bindings/Operation";
import type { OrganizerDetail } from "../bindings/OrganizerDetail";
import type { OrganizerItem } from "../bindings/OrganizerItem";
import type { OrganizerShare } from "../bindings/OrganizerShare";
import type { OrganizerUpdate } from "../bindings/OrganizerUpdate";
import type { NewRepeaterTab } from "../bindings/NewRepeaterTab";
import type { JarCookie } from "../bindings/JarCookie";
import type { RepeaterGroup } from "../bindings/RepeaterGroup";
import type { PassiveCheck } from "../bindings/PassiveCheck";
import type { CrawlConfig } from "../bindings/CrawlConfig";
import type { PluginInfo } from "../bindings/PluginInfo";
import type { PluginAction } from "../bindings/PluginAction";
import type { PluginsConfig } from "../bindings/PluginsConfig";
import type { PluginLogLine } from "../bindings/PluginLogLine";
import type { PluginPermission } from "../bindings/PluginPermission";
import type { CaptureConfig } from "../bindings/CaptureConfig";
import type { SequencerPrepared } from "../bindings/SequencerPrepared";
import type { TokenAnalysis } from "../bindings/TokenAnalysis";
import type { FuzzBuiltinList } from "../bindings/FuzzBuiltinList";
import type { FuzzConfig } from "../bindings/FuzzConfig";
import type { FuzzPreview } from "../bindings/FuzzPreview";
import type { FuzzResultQuery } from "../bindings/FuzzResultQuery";
import type { FuzzResultRow } from "../bindings/FuzzResultRow";
import type { FuzzResultsPage } from "../bindings/FuzzResultsPage";
import type { TaskDetail } from "../bindings/TaskDetail";
import type { TaskRow } from "../bindings/TaskRow";
import type { PassiveStatus } from "../bindings/PassiveStatus";
import type { RepeaterGroupSendResult } from "../bindings/RepeaterGroupSendResult";
import type { RepeaterGroupUpdate } from "../bindings/RepeaterGroupUpdate";
import type { RepeaterRedirect } from "../bindings/RepeaterRedirect";
import type { RepeaterSendResult } from "../bindings/RepeaterSendResult";
import type { RepeaterSendRow } from "../bindings/RepeaterSendRow";
import type { RepeaterSnapshot } from "../bindings/RepeaterSnapshot";
import type { RepeaterTab } from "../bindings/RepeaterTab";
import type { RepeaterTabUpdate } from "../bindings/RepeaterTabUpdate";
import type { ReportFormat } from "../bindings/ReportFormat";
import type { ReportOptions } from "../bindings/ReportOptions";
import type { SmartDecode } from "../bindings/SmartDecode";
import type { WsConnectionQuery } from "../bindings/WsConnectionQuery";
import type { WsConnectionRow } from "../bindings/WsConnectionRow";
import type { WsMessageDetail } from "../bindings/WsMessageDetail";
import type { WsMessagePage } from "../bindings/WsMessagePage";
import type { WsMessageQuery } from "../bindings/WsMessageQuery";
import type { DashboardStats } from "../bindings/DashboardStats";
import type { EngineEvent } from "../bindings/EngineEvent";
import type { ExchangeDetail } from "../bindings/ExchangeDetail";
import type { HeldDetail } from "../bindings/HeldDetail";
import type { HeldEdit } from "../bindings/HeldEdit";
import type { Highlight } from "../bindings/Highlight";
import type { HistoryPage } from "../bindings/HistoryPage";
import type { InterceptState } from "../bindings/InterceptState";
import type { LogEntry } from "../bindings/LogEntry";
import type { HistoryQuery } from "../bindings/HistoryQuery";
import type { ListenerStatus } from "../bindings/ListenerStatus";
import type { ProjectEncryption } from "../bindings/ProjectEncryption";
import type { ProjectInfo } from "../bindings/ProjectInfo";
import type { RecoveryStatus } from "../bindings/RecoveryStatus";
import type { ProxyConfig } from "../bindings/ProxyConfig";
import type { ScopeConfig } from "../bindings/ScopeConfig";
import type { ScopeDecision } from "../bindings/ScopeDecision";
import type { SearchQuery } from "../bindings/SearchQuery";
import type { SearchResults } from "../bindings/SearchResults";
import type { SiteMap } from "../bindings/SiteMap";
import type { SiteMark } from "../bindings/SiteMark";

/** Error returned by an engine command. */
export class IpcError extends Error {
  readonly kind: string;

  constructor(kind: string, message: string) {
    super(message);
    this.kind = kind;
    this.name = "IpcError";
  }
}

/** Normalizes anything thrown by `invoke` into an {@link IpcError}. */
export function toIpcError(e: unknown): IpcError {
  if (e instanceof IpcError) return e;
  if (typeof e === "object" && e !== null && "message" in e) {
    const kind = "kind" in e && typeof e.kind === "string" ? e.kind : "unknown";
    return new IpcError(kind, String(e.message));
  }
  return new IpcError("unknown", String(e));
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toIpcError(e);
  }
}

/** True when running inside the desktop shell (not a plain browser tab). */
export function runningInDesktop(): boolean {
  return isTauri();
}

export const api = {
  appInfo: () => call<AppInfo>("app_info"),
  getSettings: () => call<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) => call<AppSettings>("save_settings", { settings }),
  getProxyConfig: () => call<ProxyConfig>("get_proxy_config"),
  saveProxyConfig: (config: ProxyConfig) => call<ListenerStatus[]>("save_proxy_config", { config }),
  listListeners: () => call<ListenerStatus[]>("list_listeners"),
  startListener: (id: string) => call<null>("start_listener", { id }),
  stopListener: (id: string) => call<null>("stop_listener", { id }),
  restartListener: (id: string) => call<null>("restart_listener", { id }),
  currentProject: () => call<ProjectInfo | null>("current_project"),
  newTemporaryProject: (encrypt = false) => call<ProjectInfo>("new_temporary_project", { encrypt }),
  createProject: (path: string, password: string | null = null) =>
    call<ProjectInfo>("create_project", { path, password }),
  openProject: (path: string, password: string | null = null) =>
    call<ProjectInfo>("open_project", { path, password }),
  openProjectReadOnly: (path: string, password: string | null = null) =>
    call<ProjectInfo>("open_project_read_only", { path, password }),
  recoveryStatus: () => call<RecoveryStatus | null>("recovery_status"),
  recoveryRestore: (password: string | null = null) =>
    call<ProjectInfo>("recovery_restore", { password }),
  recoveryOpenReadOnly: (password: string | null = null) =>
    call<ProjectInfo>("recovery_open_read_only", { password }),
  recoveryDiscard: (deleteOrphanTemp = false) =>
    call<null>("recovery_discard", { deleteOrphanTemp }),
  changeProjectPassword: (password: string) => call<null>("change_project_password", { password }),
  closeProject: () => call<null>("close_project"),
  historyPage: (query: HistoryQuery) => call<HistoryPage>("history_page", { query }),
  exchangeDetail: (id: number) => call<ExchangeDetail>("exchange_detail", { id }),
  dashboardStats: () => call<DashboardStats>("dashboard_stats"),
  caStatus: () => call<CaStatus>("ca_status"),
  exportCa: (path: string, format: CertFormat) => call<string>("export_ca", { path, format }),
  regenerateCa: () => call<CaStatus>("regenerate_ca"),
  importCa: (certPath: string, keyPath: string) =>
    call<CaStatus>("import_ca", { certPath, keyPath }),
  listBrowsers: () => call<BrowserInfo[]>("list_browsers"),
  browserStatus: () => call<BrowserStatus>("browser_status"),
  launchBrowser: (browserId: string | null, url: string | null) =>
    call<BrowserStatus>("launch_browser", { browserId, url }),
  closeBrowser: () => call<null>("close_browser"),
  interceptState: () => call<InterceptState>("intercept_state"),
  setIntercept: (enabled: boolean) => call<InterceptState>("set_intercept", { enabled }),
  heldMessage: (id: number) => call<HeldDetail>("held_message", { id }),
  unpackHeld: (id: number) => call<HeldDetail>("unpack_held", { id }),
  forwardHeld: (id: number, edit: HeldEdit | null, interceptResponse: boolean) =>
    call<null>("forward_held", { id, edit, interceptResponse }),
  dropHeld: (id: number) => call<null>("drop_held", { id }),
  forwardAllHeld: () => call<null>("forward_all_held"),
  setListenerAuth: (id: string, username: string | null, password: string | null) =>
    call<ListenerStatus[]>("set_listener_auth", { id, username, password }),
  decode: (input: string, ops: Operation[]) => call<DecodeStep[]>("decode", { input, ops }),
  smartDecode: (input: string) => call<SmartDecode>("smart_decode", { input }),
  organizerAdd: (exchangeId: number) => call<number>("organizer_add", { exchangeId }),
  organizerList: () => call<OrganizerItem[]>("organizer_list"),
  organizerDetail: (id: number) => call<OrganizerDetail>("organizer_detail", { id }),
  organizerUpdate: (id: number, update: OrganizerUpdate) =>
    call<null>("organizer_update", { id, update }),
  organizerDelete: (id: number) => call<null>("organizer_delete", { id }),
  organizerExport: (
    path: string,
    collection: string | null = null,
    password: string | null = null,
  ) => call<OrganizerShare>("organizer_export", { path, collection, password }),
  organizerImport: (path: string, password: string | null = null) =>
    call<OrganizerShare>("organizer_import", { path, password }),
  isSealedFile: (path: string) => call<boolean>("is_sealed_file", { path }),
  wsConnections: (query: WsConnectionQuery) => call<WsConnectionRow[]>("ws_connections", { query }),
  wsMessages: (query: WsMessageQuery) => call<WsMessagePage>("ws_messages", { query }),
  wsMessage: (id: number) => call<WsMessageDetail>("ws_message", { id }),
  wsExport: (connectionId: number, path: string) =>
    call<string>("ws_export", { connectionId, path }),
  saveProjectAs: (path: string, encryption: ProjectEncryption = { kind: "same" }) =>
    call<ProjectInfo>("save_project_as", { path, encryption }),
  exportProject: (
    path: string,
    inScopeOnly: boolean,
    encryption: ProjectEncryption = { kind: "same" },
  ) => call<ProjectFileInfo>("export_project", { path, inScopeOnly, encryption }),
  projectSettings: () => call<ProjectSettings>("project_settings"),
  saveBackupSettings: (settings: BackupSettings) =>
    call<ProjectSettings>("save_backup_settings", { settings }),
  listBackups: () => call<BackupInfo[]>("list_backups"),
  backupNow: () => call<BackupInfo>("backup_now"),
  restoreBackup: (path: string, password: string | null = null) =>
    call<ProjectInfo>("restore_backup", { path, password }),
  scopeOverride: () => call<boolean>("scope_override"),
  setScopeOverride: (enabled: boolean) => call<ScopeConfig>("set_scope_override", { enabled }),
  projectNotes: () => call<string>("project_notes"),
  saveProjectNotes: (notes: string) => call<null>("save_project_notes", { notes }),
  auditLog: (query?: AuditQuery) => call<AuditEntry[]>("audit_log", query ? { query } : undefined),
  securityOverview: () => call<SecurityOverview>("security_overview"),
  exportSettings: (path: string, sections: SettingsSection[], password: string | null = null) =>
    call<string>("export_settings", { path, sections, password }),
  previewSettingsImport: (path: string, password: string | null = null) =>
    call<SettingsImportPreview>("preview_settings_import", { path, password }),
  applySettingsImport: (
    path: string,
    sections: SettingsSection[],
    password: string | null = null,
  ) => call<SettingsSection[]>("apply_settings_import", { path, sections, password }),
  findings: (query: FindingQuery) => call<FindingRow[]>("findings", { query }),
  passiveStatus: () => call<PassiveStatus>("passive_status"),
  passiveChecks: () => call<PassiveCheck[]>("passive_checks"),
  activeChecks: () => call<ActiveCheck[]>("active_checks"),
  passiveReanalyze: () => call<PassiveStatus>("passive_reanalyze"),
  tasks: () => call<TaskRow[]>("tasks"),
  taskDetail: (id: number) => call<TaskDetail>("task_detail", { id }),
  taskPause: (id: number) => call<null>("task_pause", { id }),
  taskResume: (id: number) => call<null>("task_resume", { id }),
  taskCancel: (id: number) => call<null>("task_cancel", { id }),
  toolConfigs: (tool: string) => call<ToolConfig[]>("tool_configs", { tool }),
  saveToolConfig: (config: ToolConfig) => call<ToolConfig[]>("save_tool_config", { config }),
  deleteToolConfig: (tool: string, name: string) =>
    call<ToolConfig[]>("delete_tool_config", { tool, name }),
  exportToolConfigs: (path: string, tool: string) =>
    call<number>("export_tool_configs", { path, tool }),
  importToolConfigs: (path: string, tool: string) =>
    call<ToolConfig[]>("import_tool_configs", { path, tool }),
  crawlGraph: (task: number) => call<CrawlGraph>("crawl_graph", { task }),
  taskSetPool: (id: number, pool: string | null) => call<null>("task_set_pool", { id, pool }),
  taskDelete: (id: number) => call<null>("task_delete", { id }),
  crawlDefaults: () => call<CrawlConfig>("crawl_defaults"),
  crawlStart: (config: CrawlConfig, pool: string | null = null) =>
    call<TaskRow>("crawl_start", { config, pool }),
  plugins: () => call<PluginInfo[]>("plugins"),
  pluginsDir: () => call<string>("plugins_dir"),
  pluginSet: (id: string, enabled: boolean, granted: PluginPermission[]) =>
    call<PluginInfo>("plugin_set", { id, enabled, granted }),
  pluginLogs: (id: string) => call<PluginLogLine[]>("plugin_logs", { id }),
  pluginActions: () => call<PluginAction[]>("plugin_actions"),
  pluginsTrustSettings: () => call<PluginsConfig>("plugins_trust_settings"),
  pluginTrust: (key: string, name: string, trust: boolean) =>
    call<null>("plugin_trust", { key, name, trust }),
  pluginsRequireSigned: (require: boolean) => call<null>("plugins_require_signed", { require }),
  pluginsInstallFrom: (path: string) => call<PluginInfo>("plugins_install_from", { path }),
  pluginRunAction: (plugin: string, action: string, exchangeId: number) =>
    call<string>("plugin_run_action", { plugin, action, exchangeId }),
  pluginsInstallExample: () => call<string>("plugins_install_example"),
  sessionRules: () => call<SessionRule[]>("session_rules"),
  saveSessionRules: (rules: SessionRule[]) => call<null>("save_session_rules", { rules }),
  credentials: () => call<CredentialInfo[]>("credentials"),
  credentialSet: (info: CredentialInfo, secret: string | null) =>
    call<CredentialInfo[]>("credential_set", { info, secret }),
  credentialDelete: (id: string) => call<CredentialInfo[]>("credential_delete", { id }),
  keyringStatus: () => call<KeyringStatus>("keyring_status"),
  keyringUnlock: (password: string) => call<KeyringStatus>("keyring_unlock", { password }),
  keyringLock: () => call<KeyringStatus>("keyring_lock"),
  keyringSetProtection: (protection: KeyringProtection, password: string | null) =>
    call<KeyringStatus>("keyring_set_protection", { protection, password }),
  scripts: () => call<ScriptLibrary>("scripts"),
  saveScripts: (config: ScriptsConfig) => call<ScriptLibrary>("save_scripts", { config }),
  exportScripts: (path: string, ids: string[]) => call<number>("export_scripts", { path, ids }),
  importScripts: (path: string) => call<ScriptLibrary>("import_scripts", { path }),
  scriptTest: (kind: ScriptKind, source: string, exchangeId: number | null) =>
    call<ScriptRun>("script_test", { kind, source, exchangeId }),
  scriptAction: (id: string, exchangeId: number) =>
    call<ScriptRun>("script_action", { id, exchangeId }),
  scriptColumn: (id: string, exchangeIds: number[]) =>
    call<ScriptCell[]>("script_column", { id, exchangeIds }),
  historyPageScripted: (query: HistoryQuery, scriptId: string) =>
    call<ScriptedHistoryPage>("history_page_scripted", { query, scriptId }),
  historyPageConditions: (query: HistoryQuery, source: string) =>
    call<ScriptedHistoryPage>("history_page_conditions", { query, source }),
  networkConfig: () => call<NetworkConfig>("network_config"),
  saveNetworkConfig: (config: NetworkConfig) =>
    call<NetworkConfig>("save_network_config", { config }),
  oastConfig: () => call<OastConfig>("oast_config"),
  oastSaveConfig: (config: OastConfig) => call<OastStatus>("oast_save_config", { config }),
  oastStatus: () => call<OastStatus>("oast_status"),
  oastTestServer: (server: OastSelfHostedConfig) => call<string>("oast_test_server", { server }),
  oastNewPayload: (note: string) => call<OastPayloadRow>("oast_new_payload", { note }),
  oastPayloads: () => call<OastPayloadRow[]>("oast_payloads"),
  oastInteractions: (correlationId: string | null) =>
    call<OastInteractionRow[]>("oast_interactions", { correlationId }),
  oastInteraction: (id: number) => call<OastInteractionDetail>("oast_interaction", { id }),
  domLaunchBrowser: (browserId: string | null, url: string | null) =>
    call<BrowserStatus>("dom_launch_browser", { browserId, url }),
  domStatus: () => call<DomStatus>("dom_status"),
  domNavigate: (url: string) => call<null>("dom_navigate", { url }),
  domEvents: () => call<DomEventRow[]>("dom_events"),
  domClear: () => call<null>("dom_clear"),
  discoveryDefaults: () => call<DiscoveryConfig>("discovery_defaults"),
  discoveryStart: (config: DiscoveryConfig, pool: string | null = null) =>
    call<TaskRow>("discovery_start", { config, pool }),
  browserCrawlStart: (config: BrowserCrawlConfig) =>
    call<TaskRow>("browser_crawl_start", { config }),
  liveTaskStart: (config: LiveTaskConfig) => call<TaskRow>("live_task_start", { config }),
  resourcePools: () => call<PoolsView>("resource_pools"),
  saveResourcePools: (config: PoolsConfig) => call<PoolsView>("save_resource_pools", { config }),
  scanConfigs: () => call<ScanConfig[]>("scan_configs"),
  saveScanConfig: (config: ScanConfig) => call<ScanConfig[]>("save_scan_config", { config }),
  deleteScanConfig: (name: string) => call<ScanConfig[]>("delete_scan_config", { name }),
  exportScanConfigs: (path: string, names: string[]) =>
    call<number>("export_scan_configs", { path, names }),
  importScanConfigs: (path: string) => call<ScanConfig[]>("import_scan_configs", { path }),
  scanStart: (launch: ScanLaunch, pool: string | null = null) =>
    call<TaskRow>("scan_start", { launch, pool }),
  apiImport: (path: string, baseUrl: string | null) =>
    call<ApiDefinitionInfo>("api_import", { path, baseUrl }),
  apiDefinitions: () => call<ApiDefinitionInfo[]>("api_definitions"),
  apiDefinition: (id: number) => call<ApiDefinitionDetail>("api_definition", { id }),
  apiDefinitionDelete: (id: number) => call<null>("api_definition_delete", { id }),
  apiRequest: (id: number, index: number) => call<ApiRequest>("api_request", { id, index }),
  apiSendStart: (config: ApiSendConfig, pool: string | null = null) =>
    call<TaskRow>("api_send_start", { config, pool }),
  graphqlIntrospect: (exchangeId: number) =>
    call<ApiDefinitionInfo>("graphql_introspect", { exchangeId }),
  graphqlImportSchema: (exchangeId: number, path: string) =>
    call<ApiDefinitionInfo>("graphql_import_schema", { exchangeId, path }),
  graphqlSchema: (id: number) => call<GraphqlSchema>("graphql_schema", { id }),
  graphqlBuild: (id: number, root: string, field: string) =>
    call<GraphqlOperation>("graphql_build", { id, root, field }),
  graphqlRequest: (id: number, query: string, variables: string) =>
    call<ApiRequest>("graphql_request", { id, query, variables }),
  siteNodeDetails: (site: SiteFilter) => call<SiteNodeDetails>("site_node_details", { site }),
  apiStatus: () => call<ApiStatus>("api_status"),
  saveApiSettings: (config: ApiConfig) => call<ApiStatus>("save_api_settings", { config }),
  regenerateApiToken: () => call<ApiStatus>("regenerate_api_token"),
  aiConfig: () => call<AiConfig>("ai_config"),
  saveAiConfig: (config: AiConfig) => call<AiConfig>("save_ai_config", { config }),
  aiPreview: (request: AiRequest) => call<AiPreview>("ai_preview", { request }),
  aiRun: (request: AiRequest, approved: string | null) =>
    call<AiAnswer>("ai_run", { request, approved }),
  aiTest: (config: AiConfig) => call<AiAnswer>("ai_test", { config }),
  aiRunStream: (request: AiRequest, approved: string | null, stream: string) =>
    call<AiAnswer>("ai_run_stream", { request, approved, stream }),
  aiCancel: (stream: string) => call<boolean>("ai_cancel", { stream }),
  loginRecordingStart: () => call<number>("login_recording_start"),
  loginRecording: () => call<number | null>("login_recording"),
  loginRecordingStop: () => call<LoginRecording>("login_recording_stop"),
  agentContinue: (task: number, message: string) =>
    call<TaskRow>("agent_continue", { task, message }),
  performance: () => call<PerformanceSnapshot>("performance"),
  tempStatus: () => call<TempStatus>("temp_status"),
  tempCleanup: () => call<TempCleanup>("temp_cleanup"),
  tempRemove: (names: string[]) => call<TempCleanup>("temp_remove", { names }),
  agentConfig: () => call<AgentConfig>("agent_config"),
  saveAgentConfig: (config: AgentConfig) => call<AgentConfig>("save_agent_config", { config }),
  agentTools: () => call<AgentToolInfo[]>("agent_tools"),
  agentStart: (start: AgentStart) => call<TaskRow>("agent_start", { start }),
  agentState: (task: number) => call<AgentState>("agent_state", { task }),
  agentApprovals: () => call<AgentApproval[]>("agent_approvals"),
  agentDecide: (approval: number, approve: boolean, note: string) =>
    call<null>("agent_decide", { approval, approve, note }),
  agentMessage: (task: number, text: string) => call<null>("agent_message", { task, text }),
  aiActivity: (task: number | null, after: number, limit: number) =>
    call<AiActivity[]>("ai_activity", { task, after, limit }),
  scanSummary: (id: number) => call<ScanSummary | null>("scan_summary", { id }),
  suppressions: () => call<SuppressionRule[]>("suppressions"),
  suppressionAdd: (rule: NewSuppression, markOpen: boolean) =>
    call<number>("suppression_add", { rule, markOpen }),
  suppressionDelete: (id: number) => call<null>("suppression_delete", { id }),
  discoveryResults: (id: number) => call<DiscoveryHit[]>("discovery_results", { id }),
  targetAnalysis: (site: SiteFilter) => call<TargetAnalysis>("target_analysis", { site }),
  snapshotSave: (name: string) => call<SiteSnapshotInfo>("snapshot_save", { name }),
  snapshots: () => call<SiteSnapshotInfo[]>("snapshots"),
  schedules: () => call<ScheduleRow[]>("schedules"),
  repeaterSetNote: (id: number, note: string) =>
    call<RepeaterTab>("repeater_set_note", { id, note }),
  repeaterSetRedirectPolicy: (id: number, policy: string) =>
    call<RepeaterTab>("repeater_set_redirect_policy", { id, policy }),
  scheduleCreate: (schedule: NewSchedule) => call<ScheduleRow>("schedule_create", { schedule }),
  scheduleSetEnabled: (id: number, enabled: boolean) =>
    call<null>("schedule_set_enabled", { id, enabled }),
  scheduleDelete: (id: number) => call<null>("schedule_delete", { id }),
  snapshotDelete: (id: number) => call<null>("snapshot_delete", { id }),
  snapshotCompare: (older: number, newer: number | null) =>
    call<SiteDiffEntry[]>("snapshot_compare", { older, newer }),
  sequencerDefaults: () => call<CaptureConfig>("sequencer_defaults"),
  sequencerPrepare: (exchangeId: number) =>
    call<SequencerPrepared>("sequencer_prepare", { exchangeId }),
  sequencerStart: (config: CaptureConfig) => call<TaskRow>("sequencer_start", { config }),
  sequencerAnalysis: (id: number) => call<TokenAnalysis>("sequencer_analysis", { id }),
  sequencerAnalyze: (tokens: string[]) => call<TokenAnalysis>("sequencer_analyze", { tokens }),
  sequencerTokens: (id: number) => call<string[]>("sequencer_tokens", { id }),
  fuzzDefaults: () => call<FuzzConfig>("fuzz_defaults"),
  fuzzBuiltinLists: () => call<FuzzBuiltinList[]>("fuzz_builtin_lists"),
  fuzzAutoMark: (text: string, kinds: PointKind[] = []) =>
    call<string>("fuzz_auto_mark", { text, kinds }),
  fuzzPreview: (config: FuzzConfig) => call<FuzzPreview>("fuzz_preview", { config }),
  fuzzStart: (config: FuzzConfig, pool: string | null = null) =>
    call<TaskRow>("fuzz_start", { config, pool }),
  fuzzResults: (id: number, query: FuzzResultQuery) =>
    call<FuzzResultsPage>("fuzz_results", { id, query }),
  fuzzSetColor: (id: number, color: Highlight | null) =>
    call<FuzzResultRow>("fuzz_set_color", { id, color }),
  fuzzExport: (id: number, path: string) => call<string>("fuzz_export", { id, path }),
  findingDetail: (id: number) => call<FindingDetail>("finding_detail", { id }),
  findingCreate: (finding: NewFinding) => call<number>("finding_create", { finding }),
  findingAddEvidence: (findingId: number, exchangeId: number, note: string) =>
    call<null>("finding_add_evidence", { findingId, exchangeId, note }),
  findingRemoveEvidence: (evidenceId: number) =>
    call<null>("finding_remove_evidence", { evidenceId }),
  findingUpdate: (id: number, update: FindingUpdate) =>
    call<null>("finding_update", { id, update }),
  findingDelete: (id: number) => call<null>("finding_delete", { id }),
  reportPreview: (options: ReportOptions) => call<string>("report_preview", { options }),
  exportReport: (options: ReportOptions, format: ReportFormat, path: string) =>
    call<string>("export_report", { options, format, path }),
  eventLog: () => call<LogEntry[]>("event_log"),
  clearEventLog: () => call<null>("clear_event_log"),
  getScope: () => call<ScopeConfig>("get_scope"),
  saveScope: (config: ScopeConfig) => call<ScopeConfig>("save_scope", { config }),
  checkScope: (url: string) => call<ScopeDecision>("check_scope", { url }),
  addUrlToScope: (url: string, exclude: boolean, path: boolean) =>
    call<ScopeConfig>("add_url_to_scope", { url, exclude, path }),
  siteMap: (query: HistoryQuery) => call<SiteMap>("site_map", { query }),
  siteMapConditions: (query: HistoryQuery, source: string) =>
    call<ScriptedSiteMap>("site_map_conditions", { query, source }),
  setSiteMark: (
    node: { scheme: string; host: string; port: number; path: string },
    mark: SiteMark | null,
  ) => call<null>("set_site_mark", { ...node, mark }),
  annotateExchange: (id: number, notes: string | null, highlight: Highlight | null) =>
    call<null>("annotate_exchange", { id, notes, highlight }),
  search: (query: SearchQuery) => call<SearchResults>("search", { query }),
  repeaterTabs: () => call<RepeaterTab[]>("repeater_tabs"),
  repeaterCreate: (tab: NewRepeaterTab) => call<RepeaterTab>("repeater_create", { tab }),
  repeaterFromExchange: (exchangeId: number) =>
    call<RepeaterTab>("repeater_from_exchange", { exchangeId }),
  repeaterUpdate: (id: number, update: RepeaterTabUpdate) =>
    call<RepeaterTab>("repeater_update", { id, update }),
  repeaterDuplicate: (id: number) => call<RepeaterTab>("repeater_duplicate", { id }),
  repeaterMove: (id: number, position: number) => call<null>("repeater_move", { id, position }),
  repeaterClose: (id: number) => call<null>("repeater_close", { id }),
  repeaterSend: (id: number, update: RepeaterTabUpdate) =>
    call<RepeaterSendResult>("repeater_send", { id, update }),
  repeaterCancel: (id: number) => call<boolean>("repeater_cancel", { id }),
  repeaterHistory: (id: number) => call<RepeaterSendRow[]>("repeater_history", { id }),
  repeaterSnapshot: (exchangeId: number) =>
    call<RepeaterSnapshot>("repeater_snapshot", { exchangeId }),
  repeaterRedirect: (exchangeId: number) =>
    call<RepeaterRedirect>("repeater_redirect", { exchangeId }),
  repeaterGroups: () => call<RepeaterGroup[]>("repeater_groups"),
  repeaterGroupCreate: (name: string, color: Highlight, tabs: number[]) =>
    call<RepeaterGroup>("repeater_group_create", { name, color, tabs }),
  repeaterGroupUpdate: (id: number, update: RepeaterGroupUpdate) =>
    call<RepeaterGroup>("repeater_group_update", { id, update }),
  repeaterGroupDelete: (id: number) => call<null>("repeater_group_delete", { id }),
  repeaterSetColor: (tab: number, color: Highlight | null) =>
    call<RepeaterTab>("repeater_set_color", { tab, color }),
  repeaterSetGroup: (tab: number, group: number | null) =>
    call<RepeaterTab>("repeater_set_group", { tab, group }),
  repeaterSendGroup: (id: number) => call<RepeaterGroupSendResult>("repeater_send_group", { id }),
  repeaterCancelGroup: (id: number) => call<boolean>("repeater_cancel_group", { id }),
  cookieJar: () => call<JarCookie[]>("cookie_jar"),
  cookieJarSetValue: (id: number, value: string) =>
    call<null>("cookie_jar_set_value", { id, value }),
  cookieJarDelete: (ids: number[]) => call<number>("cookie_jar_delete", { ids }),
  cookieJarClear: () => call<number>("cookie_jar_clear"),
};

/** A piece of a ChosAI answer that is being written. */
export interface AiDelta {
  stream: string;
  text: string;
}

/** Subscribes to the pieces of streamed ChosAI answers. */
export function onAiDelta(handler: (delta: AiDelta) => void): Promise<UnlistenFn> {
  return listen<AiDelta>("ai://delta", (e) => handler(e.payload));
}

/** Subscribes to engine events; resolves to an unsubscribe function. */
export function onEngineEvent(handler: (event: EngineEvent) => void): Promise<UnlistenFn> {
  return listen<EngineEvent>("engine://event", (e) => handler(e.payload));
}

const PROJECT_FILTER = [{ name: "ChosProxy project", extensions: ["chosproxy"] }];

const REPORT_FILTERS: Record<ReportFormat, { name: string; extensions: string[] }> = {
  html: { name: "HTML", extensions: ["html"] },
  markdown: { name: "Markdown", extensions: ["md"] },
  json: { name: "JSON", extensions: ["json"] },
};

export const dialogs = {
  /** Asks for an existing project file. Null if cancelled. */
  async chooseProjectToOpen(defaultPath?: string): Promise<string | null> {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: PROJECT_FILTER,
      defaultPath,
    });
    return typeof picked === "string" ? picked : null;
  },
  /** Asks where to create a new project file. Null if cancelled. */
  async chooseProjectToCreate(defaultPath?: string): Promise<string | null> {
    return save({ filters: PROJECT_FILTER, defaultPath });
  },
  /** Asks where to save the CA certificate. Null if cancelled. */
  async chooseCertificateDestination(format: CertFormat): Promise<string | null> {
    const ext = format === "pem" ? "pem" : "crt";
    return save({
      defaultPath: `chosproxy-ca.${ext}`,
      filters: [
        { name: format === "pem" ? "PEM certificate" : "DER certificate", extensions: [ext] },
      ],
    });
  },
  /** Asks for a PEM file (certificate or key) to read. Null if cancelled. */
  async choosePemFile(title: string): Promise<string | null> {
    const picked = await open({
      title,
      multiple: false,
      directory: false,
      filters: [{ name: "PEM", extensions: ["pem", "crt", "key"] }],
    });
    return typeof picked === "string" ? picked : null;
  },
  /** Asks for a JSON file to read. Null if cancelled. */
  async chooseJsonFile(): Promise<string | null> {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    return typeof picked === "string" ? picked : null;
  },
  /** Asks where to save an Organizer export (.chosorg / .json). Null if cancelled. */
  async chooseOrganizerDestination(defaultName: string): Promise<string | null> {
    return save({
      defaultPath: defaultName,
      filters: [
        { name: "ChosProxy Organizer export", extensions: ["chosorg", "json"] },
        { name: "JSON", extensions: ["json"] },
      ],
    });
  },
  /** Asks for an Organizer export file (.chosorg / .json) to import. Null if cancelled. */
  async chooseOrganizerFile(): Promise<string | null> {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [
        { name: "ChosProxy Organizer export", extensions: ["chosorg", "json"] },
        { name: "All files", extensions: ["*"] },
      ],
    });
    return typeof picked === "string" ? picked : null;
  },
  /** Asks where to save a settings export (.chossettings / .json). Null if cancelled. */
  async chooseSettingsDestination(defaultName: string): Promise<string | null> {
    return save({
      defaultPath: defaultName,
      filters: [
        { name: "ChosProxy settings export", extensions: ["chossettings", "json"] },
        { name: "JSON", extensions: ["json"] },
      ],
    });
  },
  /** Asks for a settings export file (.chossettings / .json) to import. Null if cancelled. */
  async chooseSettingsFile(): Promise<string | null> {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [
        { name: "ChosProxy settings export", extensions: ["chossettings", "json"] },
        { name: "All files", extensions: ["*"] },
      ],
    });
    return typeof picked === "string" ? picked : null;
  },
  /** Asks for a folder. Null if cancelled. */
  async chooseDirectory(defaultPath?: string): Promise<string | null> {
    const picked = await open({ multiple: false, directory: true, defaultPath });
    return typeof picked === "string" ? picked : null;
  },
  /** Asks where to save a JSON export. Null if cancelled. */
  /** Asks for an API definition (OpenAPI/Swagger, Postman, WSDL). Null if cancelled. */
  async chooseApiDefinition(): Promise<string | null> {
    const picked = await open({
      title: "Import an API definition",
      multiple: false,
      directory: false,
      filters: [
        { name: "API definitions", extensions: ["json", "yaml", "yml", "wsdl", "xml"] },
        { name: "All files", extensions: ["*"] },
      ],
    });
    return typeof picked === "string" ? picked : null;
  },
  async chooseCsvDestination(defaultName: string): Promise<string | null> {
    return save({ defaultPath: defaultName, filters: [{ name: "CSV", extensions: ["csv"] }] });
  },
  async chooseJsonDestination(defaultName: string): Promise<string | null> {
    return save({ defaultPath: defaultName, filters: [{ name: "JSON", extensions: ["json"] }] });
  },
  /** Asks where to save a report. Null if cancelled. */
  async chooseReportDestination(format: ReportFormat, defaultName: string): Promise<string | null> {
    const filter = REPORT_FILTERS[format];
    return save({ defaultPath: `${defaultName}.${filter.extensions[0]}`, filters: [filter] });
  },
  /** Native yes/no confirmation. */
  async confirm(message: string, title: string, okLabel: string): Promise<boolean> {
    return confirm(message, { title, kind: "warning", okLabel, cancelLabel: "Cancel" });
  },
};
