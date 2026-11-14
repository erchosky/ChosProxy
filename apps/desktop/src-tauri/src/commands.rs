//! IPC commands. Each one validates nothing itself and delegates to the
//! engine; errors are returned as `{ kind, message }`.

use crate::AppState;
use chosproxy_app::analysis::TargetAnalysis;
use chosproxy_app::{
    ActiveCheck, CaptureConfig, CrawlConfig, FindingDetail, FuzzBuiltinList, FuzzConfig,
    FuzzPreview, PassiveCheck, PassiveStatus, PluginInfo, PluginLogLine, PluginPermission,
    ReportFormat, ReportOptions, SequencerPrepared, TaskDetail, TokenAnalysis,
};
use chosproxy_app::{
    AppInfo, BackupInfo, BackupSettings, BrowserInfo, BrowserStatus, CaStatus, CertFormat,
    DashboardStats, DecodeOperation, DecodeStep, ErrorPayload, ExchangeDetail, HeldDetail,
    HeldEdit, InterceptState, ListenerStatus, LogEntry, OrganizerDetail, OrganizerShare,
    ProjectInfo, ProjectSettings, RecoveryStatus, SearchQuery, SearchResults, SecurityOverview,
    SettingsImportPreview, SettingsSection, SmartDecode, WsMessageDetail,
};
use chosproxy_app::{DiscoveryConfig, DomStatus, OastInteractionDetail, OastStatus, PointKind};
use chosproxy_app::{
    RepeaterGroupSendResult, RepeaterRedirect, RepeaterSendResult, RepeaterSnapshot,
};
use chosproxy_core::config::{AppSettings, NetworkConfig, OastConfig, ProxyConfig, SelfHostedOast};
use chosproxy_core::{ScopeConfig, ScopeDecision};
use chosproxy_storage::{
    AuditEntry, AuditQuery, Highlight, HistoryPage, HistoryQuery, OrganizerItem, OrganizerUpdate,
    ProjectFileInfo, SiteMap, SiteMark, WsConnectionQuery, WsConnectionRow, WsMessagePage,
    WsMessageQuery,
};
use chosproxy_storage::{
    DiscoveryHit, DomEventRow, OastInteractionRow, OastPayloadRow, SiteDiffEntry, SiteFilter,
    SiteSnapshotInfo,
};
use chosproxy_storage::{FindingQuery, FindingRow, FindingUpdate, NewFinding};
use chosproxy_storage::{
    FuzzResultQuery, FuzzResultRow, FuzzResultsPage, JarCookie, RepeaterGroup, RepeaterGroupUpdate,
    TaskRow,
};
use chosproxy_storage::{NewRepeaterTab, RepeaterSendRow, RepeaterTab, RepeaterTabUpdate};
use chosproxy_storage::{NewSchedule, ScheduleRow};
use std::path::PathBuf;
use tauri::State;

type CmdResult<T> = Result<T, ErrorPayload>;

fn err(e: chosproxy_app::AppError) -> ErrorPayload {
    tracing::debug!(error = %e, "command failed");
    e.payload()
}

#[tauri::command]
pub(crate) fn app_info(state: State<'_, AppState>) -> AppInfo {
    state.engine.info()
}

#[tauri::command]
pub(crate) fn get_settings(state: State<'_, AppState>) -> AppSettings {
    state.engine.settings()
}

#[tauri::command]
pub(crate) fn save_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> CmdResult<AppSettings> {
    let saved = state.engine.save_settings(settings).map_err(err)?;
    if let Some(log) = &state.log {
        log.set_level(saved.logging.level);
    }
    Ok(saved)
}

#[tauri::command]
pub(crate) async fn get_proxy_config(state: State<'_, AppState>) -> CmdResult<ProxyConfig> {
    Ok(state.engine.proxy_config().await)
}

#[tauri::command]
pub(crate) async fn save_proxy_config(
    state: State<'_, AppState>,
    config: ProxyConfig,
) -> CmdResult<Vec<ListenerStatus>> {
    state.engine.save_proxy_config(config).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn list_listeners(state: State<'_, AppState>) -> CmdResult<Vec<ListenerStatus>> {
    Ok(state.engine.listeners().await)
}

#[tauri::command]
pub(crate) async fn start_listener(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.engine.start_listener(&id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn stop_listener(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.engine.stop_listener(&id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn restart_listener(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.engine.restart_listener(&id).await.map_err(err)
}

#[tauri::command]
pub(crate) fn current_project(state: State<'_, AppState>) -> Option<ProjectInfo> {
    state.engine.project()
}

#[tauri::command]
pub(crate) async fn new_temporary_project(
    state: State<'_, AppState>,
    encrypt: Option<bool>,
) -> CmdResult<ProjectInfo> {
    state
        .engine
        .new_temporary_project_with(encrypt.unwrap_or(false))
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn create_project(
    state: State<'_, AppState>,
    path: PathBuf,
    password: Option<String>,
) -> CmdResult<ProjectInfo> {
    state
        .engine
        .create_project_with(&path, password.as_deref())
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn open_project(
    state: State<'_, AppState>,
    path: PathBuf,
    password: Option<String>,
) -> CmdResult<ProjectInfo> {
    state
        .engine
        .open_project_with(&path, password.as_deref())
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn open_project_read_only(
    state: State<'_, AppState>,
    path: PathBuf,
    password: Option<String>,
) -> CmdResult<ProjectInfo> {
    state
        .engine
        .open_project_read_only(&path, password.as_deref())
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) fn recovery_status(state: State<'_, AppState>) -> Option<RecoveryStatus> {
    state.engine.recovery_status()
}

#[tauri::command]
pub(crate) async fn recovery_restore(
    state: State<'_, AppState>,
    password: Option<String>,
) -> CmdResult<ProjectInfo> {
    state
        .engine
        .recovery_restore(password.as_deref())
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn recovery_open_read_only(
    state: State<'_, AppState>,
    password: Option<String>,
) -> CmdResult<ProjectInfo> {
    state
        .engine
        .recovery_open_read_only(password.as_deref())
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) fn recovery_discard(
    state: State<'_, AppState>,
    delete_orphan_temp: Option<bool>,
) -> CmdResult<()> {
    state
        .engine
        .recovery_discard(delete_orphan_temp.unwrap_or(false))
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn close_project(state: State<'_, AppState>) -> CmdResult<()> {
    state.engine.close_project().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn history_page(
    state: State<'_, AppState>,
    query: HistoryQuery,
) -> CmdResult<HistoryPage> {
    state.engine.history_page(query).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn exchange_detail(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<ExchangeDetail> {
    state.engine.exchange_detail(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn dashboard_stats(state: State<'_, AppState>) -> CmdResult<DashboardStats> {
    state.engine.dashboard().await.map_err(err)
}

#[tauri::command]
pub(crate) fn ca_status(state: State<'_, AppState>) -> CaStatus {
    state.engine.ca_status()
}

#[tauri::command]
pub(crate) fn export_ca(
    state: State<'_, AppState>,
    path: PathBuf,
    format: CertFormat,
) -> CmdResult<String> {
    state
        .engine
        .export_ca(&path, format)
        .map(|p| p.display().to_string())
        .map_err(err)
}

#[tauri::command]
pub(crate) fn import_ca(
    state: State<'_, AppState>,
    cert_path: String,
    key_path: String,
) -> CmdResult<CaStatus> {
    state
        .engine
        .import_ca(
            std::path::Path::new(&cert_path),
            std::path::Path::new(&key_path),
        )
        .map_err(err)
}

#[tauri::command]
pub(crate) fn regenerate_ca(state: State<'_, AppState>) -> CmdResult<CaStatus> {
    state.engine.regenerate_ca().map_err(err)
}

#[tauri::command]
pub(crate) fn list_browsers(state: State<'_, AppState>) -> Vec<BrowserInfo> {
    state.engine.browsers()
}

#[tauri::command]
pub(crate) async fn browser_status(state: State<'_, AppState>) -> CmdResult<BrowserStatus> {
    Ok(state.engine.browser_status().await)
}

#[tauri::command]
pub(crate) async fn launch_browser(
    state: State<'_, AppState>,
    browser_id: Option<String>,
    url: Option<String>,
) -> CmdResult<BrowserStatus> {
    state
        .engine
        .launch_browser(browser_id, url)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn close_browser(state: State<'_, AppState>) -> CmdResult<()> {
    state.engine.close_browser().await;
    Ok(())
}

#[tauri::command]
pub(crate) fn intercept_state(state: State<'_, AppState>) -> InterceptState {
    state.engine.intercept_state()
}

#[tauri::command]
pub(crate) fn set_intercept(state: State<'_, AppState>, enabled: bool) -> InterceptState {
    state.engine.set_intercept(enabled)
}

#[tauri::command]
pub(crate) fn held_message(state: State<'_, AppState>, id: u64) -> CmdResult<HeldDetail> {
    state.engine.held_message(id).map_err(err)
}

#[tauri::command]
pub(crate) fn unpack_held(state: State<'_, AppState>, id: u64) -> CmdResult<HeldDetail> {
    state.engine.unpack_held(id).map_err(err)
}

#[tauri::command]
pub(crate) fn forward_held(
    state: State<'_, AppState>,
    id: u64,
    edit: Option<HeldEdit>,
    intercept_response: bool,
) -> CmdResult<()> {
    state
        .engine
        .forward_held(id, edit, intercept_response)
        .map_err(err)
}

#[tauri::command]
pub(crate) fn drop_held(state: State<'_, AppState>, id: u64) -> CmdResult<()> {
    state.engine.drop_held(id).map_err(err)
}

#[tauri::command]
pub(crate) fn forward_all_held(state: State<'_, AppState>) {
    state.engine.forward_all_held();
}

#[tauri::command]
pub(crate) async fn set_listener_auth(
    state: State<'_, AppState>,
    id: String,
    username: Option<String>,
    password: Option<String>,
) -> CmdResult<Vec<ListenerStatus>> {
    let credentials = username.zip(password);
    state
        .engine
        .set_listener_auth(&id, credentials)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) fn event_log(state: State<'_, AppState>) -> Vec<LogEntry> {
    state.engine.event_log()
}

#[tauri::command]
pub(crate) fn clear_event_log(state: State<'_, AppState>) {
    state.engine.clear_event_log();
}

#[tauri::command]
pub(crate) fn decode(
    state: State<'_, AppState>,
    input: String,
    ops: Vec<DecodeOperation>,
) -> CmdResult<Vec<DecodeStep>> {
    state.engine.decode(&input, &ops).map_err(err)
}

#[tauri::command]
pub(crate) fn smart_decode(state: State<'_, AppState>, input: String) -> CmdResult<SmartDecode> {
    state.engine.smart_decode(&input).map_err(err)
}

#[tauri::command]
pub(crate) async fn organizer_add(state: State<'_, AppState>, exchange_id: i64) -> CmdResult<i64> {
    state.engine.organizer_add(exchange_id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn organizer_list(state: State<'_, AppState>) -> CmdResult<Vec<OrganizerItem>> {
    state.engine.organizer_list().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn organizer_detail(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<OrganizerDetail> {
    state.engine.organizer_detail(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn organizer_update(
    state: State<'_, AppState>,
    id: i64,
    update: OrganizerUpdate,
) -> CmdResult<()> {
    state.engine.organizer_update(id, update).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn organizer_delete(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.engine.organizer_delete(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn organizer_export(
    state: State<'_, AppState>,
    path: PathBuf,
    collection: Option<String>,
    password: Option<String>,
) -> CmdResult<OrganizerShare> {
    state
        .engine
        .organizer_export(&path, collection, password)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn organizer_import(
    state: State<'_, AppState>,
    path: PathBuf,
    password: Option<String>,
) -> CmdResult<OrganizerShare> {
    state
        .engine
        .organizer_import(&path, password)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) fn is_sealed_file(state: State<'_, AppState>, path: PathBuf) -> CmdResult<bool> {
    state.engine.is_sealed_file(&path).map_err(err)
}

#[tauri::command]
pub(crate) async fn ws_connections(
    state: State<'_, AppState>,
    query: WsConnectionQuery,
) -> CmdResult<Vec<WsConnectionRow>> {
    state.engine.ws_connections(query).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn ws_messages(
    state: State<'_, AppState>,
    query: WsMessageQuery,
) -> CmdResult<WsMessagePage> {
    state.engine.ws_messages(query).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn ws_message(state: State<'_, AppState>, id: i64) -> CmdResult<WsMessageDetail> {
    state.engine.ws_message(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn ws_export(
    state: State<'_, AppState>,
    connection_id: i64,
    path: PathBuf,
) -> CmdResult<String> {
    state
        .engine
        .ws_export(connection_id, &path)
        .await
        .map(|p| p.display().to_string())
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn save_project_as(
    state: State<'_, AppState>,
    path: PathBuf,
    encryption: Option<chosproxy_app::ProjectEncryption>,
) -> CmdResult<ProjectInfo> {
    state
        .engine
        .save_project_as_with(
            &path,
            encryption.unwrap_or(chosproxy_app::ProjectEncryption::Same),
        )
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn change_project_password(
    state: State<'_, AppState>,
    password: String,
) -> CmdResult<()> {
    state
        .engine
        .change_project_password(&password)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn export_project(
    state: State<'_, AppState>,
    path: PathBuf,
    in_scope_only: bool,
    encryption: Option<chosproxy_app::ProjectEncryption>,
) -> CmdResult<ProjectFileInfo> {
    state
        .engine
        .export_project_with(
            &path,
            in_scope_only,
            encryption.unwrap_or(chosproxy_app::ProjectEncryption::Same),
        )
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn project_settings(state: State<'_, AppState>) -> CmdResult<ProjectSettings> {
    state.engine.project_settings().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn save_backup_settings(
    state: State<'_, AppState>,
    settings: BackupSettings,
) -> CmdResult<ProjectSettings> {
    state
        .engine
        .save_backup_settings(settings)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn list_backups(state: State<'_, AppState>) -> CmdResult<Vec<BackupInfo>> {
    state.engine.list_backups().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn backup_now(state: State<'_, AppState>) -> CmdResult<BackupInfo> {
    state.engine.backup_now().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn restore_backup(
    state: State<'_, AppState>,
    path: PathBuf,
    password: Option<String>,
) -> CmdResult<ProjectInfo> {
    state
        .engine
        .restore_backup_with(&path, password.as_deref())
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) fn scope_override(state: State<'_, AppState>) -> bool {
    state.engine.scope_override()
}

#[tauri::command]
pub(crate) fn set_scope_override(
    state: State<'_, AppState>,
    enabled: bool,
) -> CmdResult<ScopeConfig> {
    state.engine.set_scope_override(enabled).map_err(err)
}

#[tauri::command]
pub(crate) async fn project_notes(state: State<'_, AppState>) -> CmdResult<String> {
    state.engine.project_notes().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn save_project_notes(state: State<'_, AppState>, notes: String) -> CmdResult<()> {
    state.engine.save_project_notes(notes).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn audit_log(
    state: State<'_, AppState>,
    query: Option<AuditQuery>,
) -> CmdResult<Vec<AuditEntry>> {
    match query {
        Some(q) => state.engine.audit_log_query(q).await.map_err(err),
        None => state.engine.audit_log(1000).await.map_err(err),
    }
}

#[tauri::command]
pub(crate) async fn security_overview(state: State<'_, AppState>) -> CmdResult<SecurityOverview> {
    Ok(state.engine.security_overview().await)
}

#[tauri::command]
pub(crate) async fn export_settings(
    state: State<'_, AppState>,
    path: PathBuf,
    sections: Vec<SettingsSection>,
    password: Option<String>,
) -> CmdResult<String> {
    state
        .engine
        .export_settings_with(&path, &sections, password.as_deref())
        .await
        .map(|p| p.display().to_string())
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn preview_settings_import(
    state: State<'_, AppState>,
    path: PathBuf,
    password: Option<String>,
) -> CmdResult<SettingsImportPreview> {
    state
        .engine
        .preview_settings_import_with(&path, password.as_deref())
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn apply_settings_import(
    state: State<'_, AppState>,
    path: PathBuf,
    sections: Vec<SettingsSection>,
    password: Option<String>,
) -> CmdResult<Vec<SettingsSection>> {
    state
        .engine
        .apply_settings_import_with(&path, &sections, password.as_deref())
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn findings(
    state: State<'_, AppState>,
    query: FindingQuery,
) -> CmdResult<Vec<FindingRow>> {
    state.engine.findings(query).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn finding_detail(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<FindingDetail> {
    state.engine.finding_detail(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn finding_create(
    state: State<'_, AppState>,
    finding: NewFinding,
) -> CmdResult<i64> {
    state.engine.finding_create(finding).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn finding_add_evidence(
    state: State<'_, AppState>,
    finding_id: i64,
    exchange_id: i64,
    note: String,
) -> CmdResult<()> {
    state
        .engine
        .finding_add_evidence(finding_id, exchange_id, note)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn finding_remove_evidence(
    state: State<'_, AppState>,
    evidence_id: i64,
) -> CmdResult<()> {
    state
        .engine
        .finding_remove_evidence(evidence_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn finding_update(
    state: State<'_, AppState>,
    id: i64,
    update: FindingUpdate,
) -> CmdResult<()> {
    state.engine.finding_update(id, update).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn finding_delete(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.engine.finding_delete(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn report_preview(
    state: State<'_, AppState>,
    options: ReportOptions,
) -> CmdResult<String> {
    state.engine.report_preview(options).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn export_report(
    state: State<'_, AppState>,
    options: ReportOptions,
    format: ReportFormat,
    path: PathBuf,
) -> CmdResult<String> {
    state
        .engine
        .export_report(options, format, &path)
        .await
        .map(|p| p.display().to_string())
        .map_err(err)
}

#[tauri::command]
pub(crate) fn get_scope(state: State<'_, AppState>) -> ScopeConfig {
    state.engine.scope_config()
}

#[tauri::command]
pub(crate) fn save_scope(
    state: State<'_, AppState>,
    config: ScopeConfig,
) -> CmdResult<ScopeConfig> {
    state.engine.save_scope_config(config).map_err(err)
}

#[tauri::command]
pub(crate) fn check_scope(state: State<'_, AppState>, url: String) -> CmdResult<ScopeDecision> {
    state.engine.check_scope(&url).map_err(err)
}

#[tauri::command]
pub(crate) fn add_url_to_scope(
    state: State<'_, AppState>,
    url: String,
    exclude: bool,
    path: bool,
) -> CmdResult<ScopeConfig> {
    state
        .engine
        .add_url_to_scope(&url, exclude, path)
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn site_map(
    state: State<'_, AppState>,
    query: HistoryQuery,
) -> CmdResult<SiteMap> {
    state.engine.site_map(query).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn set_site_mark(
    state: State<'_, AppState>,
    scheme: String,
    host: String,
    port: u16,
    path: String,
    mark: Option<SiteMark>,
) -> CmdResult<()> {
    state
        .engine
        .set_site_mark(scheme, host, port, path, mark)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn annotate_exchange(
    state: State<'_, AppState>,
    id: i64,
    notes: Option<String>,
    highlight: Option<Highlight>,
) -> CmdResult<()> {
    state
        .engine
        .annotate_exchange(id, notes, highlight)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn search(
    state: State<'_, AppState>,
    query: SearchQuery,
) -> CmdResult<SearchResults> {
    state.engine.search(query).await.map_err(err)
}

// --------------------------------------------------------------------------
// Repeater

#[tauri::command]
pub(crate) async fn repeater_tabs(state: State<'_, AppState>) -> CmdResult<Vec<RepeaterTab>> {
    state.engine.repeater_tabs().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_create(
    state: State<'_, AppState>,
    tab: NewRepeaterTab,
) -> CmdResult<RepeaterTab> {
    state.engine.repeater_create(tab).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_from_exchange(
    state: State<'_, AppState>,
    exchange_id: i64,
) -> CmdResult<RepeaterTab> {
    state
        .engine
        .repeater_from_exchange(exchange_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_update(
    state: State<'_, AppState>,
    id: i64,
    update: RepeaterTabUpdate,
) -> CmdResult<RepeaterTab> {
    state.engine.repeater_update(id, update).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_duplicate(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<RepeaterTab> {
    state.engine.repeater_duplicate(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_move(
    state: State<'_, AppState>,
    id: i64,
    position: u32,
) -> CmdResult<()> {
    state.engine.repeater_move(id, position).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_close(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.engine.repeater_close(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_send(
    state: State<'_, AppState>,
    id: i64,
    update: RepeaterTabUpdate,
) -> CmdResult<RepeaterSendResult> {
    state.engine.repeater_send(id, update).await.map_err(err)
}

#[tauri::command]
pub(crate) fn repeater_cancel(state: State<'_, AppState>, id: i64) -> bool {
    state.engine.repeater_cancel(id)
}

#[tauri::command]
pub(crate) async fn repeater_history(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<Vec<RepeaterSendRow>> {
    state.engine.repeater_history(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_snapshot(
    state: State<'_, AppState>,
    exchange_id: i64,
) -> CmdResult<RepeaterSnapshot> {
    state
        .engine
        .repeater_snapshot(exchange_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_redirect(
    state: State<'_, AppState>,
    exchange_id: i64,
) -> CmdResult<RepeaterRedirect> {
    state
        .engine
        .repeater_redirect(exchange_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_groups(state: State<'_, AppState>) -> CmdResult<Vec<RepeaterGroup>> {
    state.engine.repeater_groups().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_group_create(
    state: State<'_, AppState>,
    name: String,
    color: Highlight,
    tabs: Vec<i64>,
) -> CmdResult<RepeaterGroup> {
    state
        .engine
        .repeater_group_create(name, color, tabs)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_group_update(
    state: State<'_, AppState>,
    id: i64,
    update: RepeaterGroupUpdate,
) -> CmdResult<RepeaterGroup> {
    state
        .engine
        .repeater_group_update(id, update)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_group_delete(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.engine.repeater_group_delete(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_set_color(
    state: State<'_, AppState>,
    tab: i64,
    color: Option<chosproxy_storage::Highlight>,
) -> CmdResult<RepeaterTab> {
    state
        .engine
        .repeater_set_color(tab, color)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_set_group(
    state: State<'_, AppState>,
    tab: i64,
    group: Option<i64>,
) -> CmdResult<RepeaterTab> {
    state
        .engine
        .repeater_set_group(tab, group)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_send_group(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<RepeaterGroupSendResult> {
    state.engine.repeater_send_group(id).await.map_err(err)
}

#[tauri::command]
pub(crate) fn repeater_cancel_group(state: State<'_, AppState>, id: i64) -> bool {
    state.engine.repeater_cancel_group(id)
}

#[tauri::command]
pub(crate) async fn cookie_jar(state: State<'_, AppState>) -> CmdResult<Vec<JarCookie>> {
    state.engine.cookie_jar().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn cookie_jar_set_value(
    state: State<'_, AppState>,
    id: i64,
    value: String,
) -> CmdResult<()> {
    state
        .engine
        .cookie_jar_set_value(id, value)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn cookie_jar_delete(
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> CmdResult<usize> {
    state.engine.cookie_jar_delete(ids).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn cookie_jar_clear(state: State<'_, AppState>) -> CmdResult<usize> {
    state.engine.cookie_jar_clear().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn passive_status(state: State<'_, AppState>) -> CmdResult<PassiveStatus> {
    state.engine.passive_status().await.map_err(err)
}

#[tauri::command]
pub(crate) fn passive_checks(state: State<'_, AppState>) -> Vec<PassiveCheck> {
    state.engine.passive_checks()
}

#[tauri::command]
pub(crate) fn active_checks(state: State<'_, AppState>) -> Vec<ActiveCheck> {
    state.engine.active_checks()
}

#[tauri::command]
pub(crate) async fn passive_reanalyze(state: State<'_, AppState>) -> CmdResult<PassiveStatus> {
    state.engine.passive_reanalyze().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn tasks(state: State<'_, AppState>) -> CmdResult<Vec<TaskRow>> {
    state.engine.tasks().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn task_detail(state: State<'_, AppState>, id: i64) -> CmdResult<TaskDetail> {
    state.engine.task_detail(id).await.map_err(err)
}

#[tauri::command]
pub(crate) fn task_pause(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.engine.task_pause(id).map_err(err)
}

#[tauri::command]
pub(crate) fn task_resume(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.engine.task_resume(id).map_err(err)
}

#[tauri::command]
pub(crate) async fn crawl_graph(
    state: State<'_, AppState>,
    task: i64,
) -> CmdResult<chosproxy_app::CrawlGraph> {
    state.engine.crawl_graph(task).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn task_set_pool(
    state: State<'_, AppState>,
    id: i64,
    pool: Option<String>,
) -> CmdResult<()> {
    state.engine.task_set_pool(id, pool).await.map_err(err)
}

#[tauri::command]
pub(crate) fn task_cancel(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.engine.task_cancel(id).map_err(err)
}

#[tauri::command]
pub(crate) async fn task_delete(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.engine.task_delete(id).await.map_err(err)
}

#[tauri::command]
pub(crate) fn crawl_defaults(state: State<'_, AppState>) -> CrawlConfig {
    state.engine.crawl_defaults()
}

#[tauri::command]
pub(crate) async fn crawl_start(
    state: State<'_, AppState>,
    config: CrawlConfig,
    pool: Option<String>,
) -> CmdResult<TaskRow> {
    let engine = &state.engine;
    engine
        .with_launch_pool(pool, engine.crawl_start(config))
        .await
        .and_then(|r| r)
        .map_err(err)
}

#[tauri::command]
pub(crate) fn fuzz_defaults(state: State<'_, AppState>) -> FuzzConfig {
    state.engine.fuzz_defaults()
}

#[tauri::command]
pub(crate) fn fuzz_builtin_lists(state: State<'_, AppState>) -> Vec<FuzzBuiltinList> {
    state.engine.fuzz_builtin_lists()
}

#[tauri::command]
pub(crate) fn fuzz_auto_mark(
    state: State<'_, AppState>,
    text: String,
    kinds: Vec<PointKind>,
) -> String {
    state.engine.fuzz_auto_mark(&text, &kinds)
}

#[tauri::command]
pub(crate) fn fuzz_preview(state: State<'_, AppState>, config: FuzzConfig) -> FuzzPreview {
    state.engine.fuzz_preview(&config)
}

#[tauri::command]
pub(crate) async fn fuzz_start(
    state: State<'_, AppState>,
    config: FuzzConfig,
    pool: Option<String>,
) -> CmdResult<TaskRow> {
    let engine = &state.engine;
    engine
        .with_launch_pool(pool, engine.fuzz_start(config))
        .await
        .and_then(|r| r)
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn fuzz_results(
    state: State<'_, AppState>,
    id: i64,
    query: FuzzResultQuery,
) -> CmdResult<FuzzResultsPage> {
    state.engine.fuzz_results(id, query).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn fuzz_set_color(
    state: State<'_, AppState>,
    id: i64,
    color: Option<Highlight>,
) -> CmdResult<FuzzResultRow> {
    state.engine.fuzz_set_color(id, color).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn fuzz_export(
    state: State<'_, AppState>,
    id: i64,
    path: PathBuf,
) -> CmdResult<String> {
    state
        .engine
        .fuzz_export_csv(id, &path)
        .await
        .map(|p| p.display().to_string())
        .map_err(err)
}

#[tauri::command]
pub(crate) fn sequencer_defaults(state: State<'_, AppState>) -> CaptureConfig {
    state.engine.sequencer_defaults()
}

#[tauri::command]
pub(crate) async fn sequencer_prepare(
    state: State<'_, AppState>,
    exchange_id: i64,
) -> CmdResult<SequencerPrepared> {
    state
        .engine
        .sequencer_prepare(exchange_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn sequencer_start(
    state: State<'_, AppState>,
    config: CaptureConfig,
) -> CmdResult<TaskRow> {
    state.engine.sequencer_start(config).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn sequencer_analysis(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<TokenAnalysis> {
    state.engine.sequencer_analysis(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn sequencer_analyze(
    state: State<'_, AppState>,
    tokens: Vec<String>,
) -> CmdResult<TokenAnalysis> {
    state.engine.sequencer_analyze(tokens).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn sequencer_tokens(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<Vec<String>> {
    state.engine.sequencer_tokens(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn plugins(state: State<'_, AppState>) -> CmdResult<Vec<PluginInfo>> {
    state.engine.plugins().await.map_err(err)
}

#[tauri::command]
pub(crate) fn plugins_dir(state: State<'_, AppState>) -> String {
    state.engine.plugins_dir().display().to_string()
}

#[tauri::command]
pub(crate) async fn plugin_set(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
    granted: Vec<PluginPermission>,
) -> CmdResult<PluginInfo> {
    state
        .engine
        .plugin_set(&id, enabled, granted)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) fn plugin_logs(state: State<'_, AppState>, id: String) -> Vec<PluginLogLine> {
    state.engine.plugin_logs(&id)
}

#[tauri::command]
pub(crate) fn plugins_trust_settings(
    state: State<'_, AppState>,
) -> chosproxy_core::config::PluginsConfig {
    state.engine.plugins_trust_settings()
}

#[tauri::command]
pub(crate) async fn plugin_trust(
    state: State<'_, AppState>,
    key: String,
    name: String,
    trust: bool,
) -> CmdResult<()> {
    state
        .engine
        .plugin_trust(key, name, trust)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn plugins_require_signed(
    state: State<'_, AppState>,
    require: bool,
) -> CmdResult<()> {
    state
        .engine
        .plugins_require_signed(require)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn plugins_install_from(
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<chosproxy_app::PluginInfo> {
    state
        .engine
        .plugins_install_from(std::path::Path::new(&path))
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) fn plugin_actions(state: State<'_, AppState>) -> Vec<chosproxy_app::PluginAction> {
    state.engine.plugin_actions()
}

#[tauri::command]
pub(crate) async fn plugin_run_action(
    state: State<'_, AppState>,
    plugin: String,
    action: String,
    exchange_id: i64,
) -> CmdResult<String> {
    state
        .engine
        .plugin_run_action(&plugin, &action, exchange_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn plugins_install_example(state: State<'_, AppState>) -> CmdResult<String> {
    state
        .engine
        .plugins_install_example()
        .await
        .map(|p| p.display().to_string())
        .map_err(err)
}

// ---------------------------------------------------------------- Sessions

#[tauri::command]
pub(crate) async fn session_rules(
    state: State<'_, AppState>,
) -> CmdResult<Vec<chosproxy_app::SessionRule>> {
    state.engine.session_rules().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn save_session_rules(
    state: State<'_, AppState>,
    rules: Vec<chosproxy_app::SessionRule>,
) -> CmdResult<()> {
    state.engine.save_session_rules(rules).await.map_err(err)
}

// ---------------------------------------------------------------- Keyring

#[tauri::command]
pub(crate) async fn credentials(
    state: State<'_, AppState>,
) -> CmdResult<Vec<chosproxy_app::CredentialInfo>> {
    state.engine.credentials().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn credential_set(
    state: State<'_, AppState>,
    info: chosproxy_app::CredentialInfo,
    secret: Option<String>,
) -> CmdResult<Vec<chosproxy_app::CredentialInfo>> {
    state.engine.credential_set(info, secret).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn keyring_status(
    state: State<'_, AppState>,
) -> CmdResult<chosproxy_app::KeyringStatus> {
    state.engine.keyring_status().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn keyring_unlock(
    state: State<'_, AppState>,
    password: String,
) -> CmdResult<chosproxy_app::KeyringStatus> {
    state.engine.keyring_unlock(password).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn keyring_lock(
    state: State<'_, AppState>,
) -> CmdResult<chosproxy_app::KeyringStatus> {
    state.engine.keyring_lock().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn keyring_set_protection(
    state: State<'_, AppState>,
    protection: chosproxy_app::KeyringProtection,
    password: Option<String>,
) -> CmdResult<chosproxy_app::KeyringStatus> {
    state
        .engine
        .keyring_set_protection(protection, password)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn credential_delete(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<Vec<chosproxy_app::CredentialInfo>> {
    state.engine.credential_delete(&id).await.map_err(err)
}

// ---------------------------------------------------------------- Scripts

#[tauri::command]
pub(crate) fn scripts(state: State<'_, AppState>) -> chosproxy_app::ScriptLibrary {
    state.engine.scripts()
}

#[tauri::command]
pub(crate) async fn save_scripts(
    state: State<'_, AppState>,
    config: chosproxy_core::config::ScriptsConfig,
) -> CmdResult<chosproxy_app::ScriptLibrary> {
    state.engine.save_scripts(config).await.map_err(err)
}

#[tauri::command]
pub(crate) fn export_scripts(
    state: State<'_, AppState>,
    path: String,
    ids: Vec<String>,
) -> CmdResult<usize> {
    state
        .engine
        .export_scripts(std::path::Path::new(&path), &ids)
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn import_scripts(
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<chosproxy_app::ScriptLibrary> {
    state
        .engine
        .import_scripts(std::path::Path::new(&path))
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn script_test(
    state: State<'_, AppState>,
    kind: chosproxy_core::config::ScriptKind,
    source: String,
    exchange_id: Option<i64>,
) -> CmdResult<chosproxy_app::ScriptRun> {
    state
        .engine
        .script_test(kind, source, exchange_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn script_action(
    state: State<'_, AppState>,
    id: String,
    exchange_id: i64,
) -> CmdResult<chosproxy_app::ScriptRun> {
    state
        .engine
        .script_action(&id, exchange_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn script_column(
    state: State<'_, AppState>,
    id: String,
    exchange_ids: Vec<i64>,
) -> CmdResult<Vec<chosproxy_app::ScriptCell>> {
    state
        .engine
        .script_column(&id, exchange_ids)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn site_map_conditions(
    state: State<'_, AppState>,
    query: chosproxy_storage::HistoryQuery,
    source: String,
) -> CmdResult<chosproxy_app::ScriptedSiteMap> {
    state
        .engine
        .site_map_conditions(query, source)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn history_page_conditions(
    state: State<'_, AppState>,
    query: chosproxy_storage::HistoryQuery,
    source: String,
) -> CmdResult<chosproxy_app::ScriptedHistoryPage> {
    state
        .engine
        .history_page_conditions(query, source)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn history_page_scripted(
    state: State<'_, AppState>,
    query: chosproxy_storage::HistoryQuery,
    script_id: String,
) -> CmdResult<chosproxy_app::ScriptedHistoryPage> {
    state
        .engine
        .history_page_scripted(query, &script_id)
        .await
        .map_err(err)
}

// ---------------------------------------------------------------- Network

#[tauri::command]
pub(crate) fn network_config(state: State<'_, AppState>) -> NetworkConfig {
    state.engine.network_config()
}

#[tauri::command]
pub(crate) fn save_network_config(
    state: State<'_, AppState>,
    config: NetworkConfig,
) -> CmdResult<NetworkConfig> {
    state.engine.save_network_config(config).map_err(err)
}

// ---------------------------------------------------------------- OAST

#[tauri::command]
pub(crate) fn oast_config(state: State<'_, AppState>) -> OastConfig {
    state.engine.oast_config()
}

#[tauri::command]
pub(crate) async fn oast_save_config(
    state: State<'_, AppState>,
    config: OastConfig,
) -> CmdResult<OastStatus> {
    state.engine.oast_save_config(config).await.map_err(err)
}

#[tauri::command]
pub(crate) fn oast_status(state: State<'_, AppState>) -> OastStatus {
    state.engine.oast_status()
}

#[tauri::command]
pub(crate) async fn oast_test_server(
    state: State<'_, AppState>,
    server: SelfHostedOast,
) -> CmdResult<String> {
    state.engine.oast_test_server(server).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn oast_new_payload(
    state: State<'_, AppState>,
    note: String,
) -> CmdResult<OastPayloadRow> {
    state.engine.oast_new_payload(note).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn oast_payloads(state: State<'_, AppState>) -> CmdResult<Vec<OastPayloadRow>> {
    state.engine.oast_payloads().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn oast_interactions(
    state: State<'_, AppState>,
    correlation_id: Option<String>,
) -> CmdResult<Vec<OastInteractionRow>> {
    state
        .engine
        .oast_interactions(correlation_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn oast_interaction(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<OastInteractionDetail> {
    state.engine.oast_interaction(id).await.map_err(err)
}

// ---------------------------------------------------------------- DOM Inspector

#[tauri::command]
pub(crate) async fn dom_launch_browser(
    state: State<'_, AppState>,
    browser_id: Option<String>,
    url: Option<String>,
) -> CmdResult<BrowserStatus> {
    state
        .engine
        .launch_browser_with(browser_id, url, true)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn dom_navigate(state: State<'_, AppState>, url: String) -> CmdResult<()> {
    state.engine.dom_navigate(url).await.map_err(err)
}

#[tauri::command]
pub(crate) fn dom_status(state: State<'_, AppState>) -> DomStatus {
    state.engine.dom_status()
}

#[tauri::command]
pub(crate) async fn dom_events(state: State<'_, AppState>) -> CmdResult<Vec<DomEventRow>> {
    state.engine.dom_events().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn dom_clear(state: State<'_, AppState>) -> CmdResult<()> {
    state.engine.dom_clear().await.map_err(err)
}

// ---------------------------------------------------------------- Content discovery

#[tauri::command]
pub(crate) fn discovery_defaults(state: State<'_, AppState>) -> DiscoveryConfig {
    state.engine.discovery_defaults()
}

#[tauri::command]
pub(crate) async fn browser_crawl_start(
    state: State<'_, AppState>,
    config: chosproxy_app::BrowserCrawlConfig,
) -> CmdResult<TaskRow> {
    state.engine.browser_crawl_start(config).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn discovery_start(
    state: State<'_, AppState>,
    config: DiscoveryConfig,
    pool: Option<String>,
) -> CmdResult<TaskRow> {
    let engine = &state.engine;
    engine
        .with_launch_pool(pool, engine.discovery_start(config))
        .await
        .and_then(|r| r)
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn discovery_results(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<Vec<DiscoveryHit>> {
    state.engine.discovery_results(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn target_analysis(
    state: State<'_, AppState>,
    site: SiteFilter,
) -> CmdResult<TargetAnalysis> {
    state.engine.target_analysis(site).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn snapshot_save(
    state: State<'_, AppState>,
    name: String,
) -> CmdResult<SiteSnapshotInfo> {
    state.engine.snapshot_save(name).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn snapshots(state: State<'_, AppState>) -> CmdResult<Vec<SiteSnapshotInfo>> {
    state.engine.snapshots().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn snapshot_delete(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.engine.snapshot_delete(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn snapshot_compare(
    state: State<'_, AppState>,
    older: i64,
    newer: Option<i64>,
) -> CmdResult<Vec<SiteDiffEntry>> {
    state
        .engine
        .snapshot_compare(older, newer)
        .await
        .map_err(err)
}

// ---------------------------------------------------------------- Schedules

#[tauri::command]
pub(crate) async fn schedules(state: State<'_, AppState>) -> CmdResult<Vec<ScheduleRow>> {
    state.engine.schedules().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn schedule_create(
    state: State<'_, AppState>,
    schedule: NewSchedule,
) -> CmdResult<ScheduleRow> {
    state.engine.schedule_create(schedule).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn schedule_set_enabled(
    state: State<'_, AppState>,
    id: i64,
    enabled: bool,
) -> CmdResult<()> {
    state
        .engine
        .schedule_set_enabled(id, enabled)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn schedule_delete(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.engine.schedule_delete(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_set_note(
    state: State<'_, AppState>,
    id: i64,
    note: String,
) -> CmdResult<RepeaterTab> {
    state.engine.repeater_set_note(id, note).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn repeater_set_redirect_policy(
    state: State<'_, AppState>,
    id: i64,
    policy: String,
) -> CmdResult<RepeaterTab> {
    state
        .engine
        .repeater_set_redirect_policy(id, policy)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn live_task_start(
    state: State<'_, AppState>,
    config: chosproxy_app::LiveTaskConfig,
) -> CmdResult<TaskRow> {
    state.engine.live_task_start(config).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn scan_summary(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<Option<chosproxy_app::ScanSummary>> {
    state.engine.scan_summary(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn resource_pools(
    state: State<'_, AppState>,
) -> CmdResult<chosproxy_app::PoolsView> {
    Ok(state.engine.resource_pools().await)
}

#[tauri::command]
pub(crate) async fn save_resource_pools(
    state: State<'_, AppState>,
    config: chosproxy_app::PoolsConfig,
) -> CmdResult<chosproxy_app::PoolsView> {
    state.engine.save_resource_pools(config).await.map_err(err)
}

#[tauri::command]
pub(crate) fn scan_configs(state: State<'_, AppState>) -> Vec<chosproxy_app::ScanConfig> {
    state.engine.scan_configs()
}

#[tauri::command]
pub(crate) fn save_scan_config(
    state: State<'_, AppState>,
    config: chosproxy_app::ScanConfig,
) -> CmdResult<Vec<chosproxy_app::ScanConfig>> {
    state.engine.save_scan_config(config).map_err(err)
}

#[tauri::command]
pub(crate) fn delete_scan_config(
    state: State<'_, AppState>,
    name: String,
) -> CmdResult<Vec<chosproxy_app::ScanConfig>> {
    state.engine.delete_scan_config(&name).map_err(err)
}

#[tauri::command]
pub(crate) fn tool_configs(
    state: State<'_, AppState>,
    tool: String,
) -> Vec<chosproxy_app::ToolConfig> {
    state.engine.tool_configs(&tool)
}

#[tauri::command]
pub(crate) fn save_tool_config(
    state: State<'_, AppState>,
    config: chosproxy_app::ToolConfig,
) -> CmdResult<Vec<chosproxy_app::ToolConfig>> {
    state.engine.save_tool_config(config).map_err(err)
}

#[tauri::command]
pub(crate) fn delete_tool_config(
    state: State<'_, AppState>,
    tool: String,
    name: String,
) -> CmdResult<Vec<chosproxy_app::ToolConfig>> {
    state.engine.delete_tool_config(&tool, &name).map_err(err)
}

#[tauri::command]
pub(crate) fn export_tool_configs(
    state: State<'_, AppState>,
    path: String,
    tool: String,
) -> CmdResult<usize> {
    state
        .engine
        .export_tool_configs(std::path::Path::new(&path), &tool)
        .map_err(err)
}

#[tauri::command]
pub(crate) fn import_tool_configs(
    state: State<'_, AppState>,
    path: String,
    tool: String,
) -> CmdResult<Vec<chosproxy_app::ToolConfig>> {
    state
        .engine
        .import_tool_configs(std::path::Path::new(&path), &tool)
        .map_err(err)
}

#[tauri::command]
pub(crate) fn export_scan_configs(
    state: State<'_, AppState>,
    path: String,
    names: Vec<String>,
) -> CmdResult<usize> {
    state
        .engine
        .export_scan_configs(std::path::Path::new(&path), &names)
        .map_err(err)
}

#[tauri::command]
pub(crate) fn import_scan_configs(
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<Vec<chosproxy_app::ScanConfig>> {
    state
        .engine
        .import_scan_configs(std::path::Path::new(&path))
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn scan_start(
    state: State<'_, AppState>,
    launch: chosproxy_app::ScanLaunch,
    pool: Option<String>,
) -> CmdResult<TaskRow> {
    let engine = &state.engine;
    engine
        .with_launch_pool(pool, engine.scan_start(launch))
        .await
        .and_then(|r| r)
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn suppressions(
    state: State<'_, AppState>,
) -> CmdResult<Vec<chosproxy_storage::SuppressionRule>> {
    state.engine.suppressions().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn suppression_add(
    state: State<'_, AppState>,
    rule: chosproxy_storage::NewSuppression,
    mark_open: bool,
) -> CmdResult<u32> {
    state
        .engine
        .suppression_add(rule, mark_open)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn suppression_delete(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.engine.suppression_delete(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn api_import(
    state: State<'_, AppState>,
    path: String,
    base_url: Option<String>,
) -> CmdResult<chosproxy_storage::ApiDefinitionInfo> {
    state
        .engine
        .api_import(std::path::Path::new(&path), base_url)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn api_definitions(
    state: State<'_, AppState>,
) -> CmdResult<Vec<chosproxy_storage::ApiDefinitionInfo>> {
    state.engine.api_definitions().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn api_definition(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<chosproxy_app::ApiDefinitionDetail> {
    state.engine.api_definition(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn api_definition_delete(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.engine.api_definition_delete(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn api_request(
    state: State<'_, AppState>,
    id: i64,
    index: u32,
) -> CmdResult<chosproxy_app::ApiRequest> {
    state.engine.api_request(id, index).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn api_send_start(
    state: State<'_, AppState>,
    config: chosproxy_app::ApiSendConfig,
    pool: Option<String>,
) -> CmdResult<TaskRow> {
    let engine = &state.engine;
    engine
        .with_launch_pool(pool, engine.api_send_start(config))
        .await
        .and_then(|r| r)
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn graphql_introspect(
    state: State<'_, AppState>,
    exchange_id: i64,
) -> CmdResult<chosproxy_storage::ApiDefinitionInfo> {
    state
        .engine
        .graphql_introspect(exchange_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn graphql_import_schema(
    state: State<'_, AppState>,
    exchange_id: i64,
    path: String,
) -> CmdResult<chosproxy_storage::ApiDefinitionInfo> {
    state
        .engine
        .graphql_import_schema(exchange_id, std::path::Path::new(&path))
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn graphql_schema(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<chosproxy_apispec::graphql::GqlSchema> {
    state.engine.graphql_schema(id).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn graphql_build(
    state: State<'_, AppState>,
    id: i64,
    root: String,
    field: String,
) -> CmdResult<chosproxy_apispec::graphql::GqlOperation> {
    state
        .engine
        .graphql_build(id, &root, &field)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn graphql_request(
    state: State<'_, AppState>,
    id: i64,
    query: String,
    variables: String,
) -> CmdResult<chosproxy_app::ApiRequest> {
    state
        .engine
        .graphql_request(id, &query, &variables)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn site_node_details(
    state: State<'_, AppState>,
    site: chosproxy_storage::SiteFilter,
) -> CmdResult<chosproxy_app::SiteNodeDetails> {
    state.engine.site_node_details(site).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn api_status(state: State<'_, AppState>) -> CmdResult<chosproxy_app::ApiStatus> {
    Ok(state.automation.status(&state.engine).await)
}

#[tauri::command]
pub(crate) async fn save_api_settings(
    state: State<'_, AppState>,
    config: chosproxy_core::config::ApiConfig,
) -> CmdResult<chosproxy_app::ApiStatus> {
    state.engine.save_api_config(config).map_err(err)?;
    Ok(state.automation.restart(&state.engine).await)
}

#[tauri::command]
pub(crate) async fn regenerate_api_token(
    state: State<'_, AppState>,
) -> CmdResult<chosproxy_app::ApiStatus> {
    chosproxy_api::token::regenerate(&state.engine.paths().data_dir)
        .map_err(|e| err(chosproxy_app::AppError::Io(e)))?;
    state
        .engine
        .audit_automation("token", "API token regenerated");
    Ok(state.automation.restart(&state.engine).await)
}

#[tauri::command]
pub(crate) fn ai_config(state: State<'_, AppState>) -> chosproxy_core::config::AiConfig {
    state.engine.ai_config()
}

#[tauri::command]
pub(crate) fn save_ai_config(
    state: State<'_, AppState>,
    config: chosproxy_core::config::AiConfig,
) -> CmdResult<chosproxy_core::config::AiConfig> {
    state.engine.save_ai_config(config).map_err(err)
}

#[tauri::command]
pub(crate) async fn ai_preview(
    state: State<'_, AppState>,
    request: chosproxy_app::AiRequest,
) -> CmdResult<chosproxy_app::AiPreview> {
    state.engine.ai_preview(&request).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn ai_run(
    state: State<'_, AppState>,
    request: chosproxy_app::AiRequest,
    approved: Option<String>,
) -> CmdResult<chosproxy_app::AiAnswer> {
    state
        .engine
        .ai_run(&request, approved.as_deref())
        .await
        .map_err(err)
}

/// A piece of a streamed ChosAI answer.
#[derive(Clone, serde::Serialize)]
struct AiDelta<'a> {
    stream: &'a str,
    text: &'a str,
}

/// Asks ChosAI and emits the answer piece by piece as `ai://delta` events
/// (`{stream, text}`) while the model writes it.
#[tauri::command]
pub(crate) async fn ai_run_stream(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: chosproxy_app::AiRequest,
    approved: Option<String>,
    stream: String,
) -> CmdResult<chosproxy_app::AiAnswer> {
    use tauri::Emitter;
    state
        .engine
        .ai_run_stream(&request, approved.as_deref(), &stream, |text| {
            let _ = app.emit(
                "ai://delta",
                AiDelta {
                    stream: &stream,
                    text,
                },
            );
        })
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) fn ai_cancel(state: State<'_, AppState>, stream: String) -> bool {
    state.engine.ai_cancel(&stream)
}

#[tauri::command]
pub(crate) async fn login_recording_start(state: State<'_, AppState>) -> CmdResult<i64> {
    state.engine.login_recording_start().await.map_err(err)
}

#[tauri::command]
pub(crate) fn login_recording(state: State<'_, AppState>) -> Option<i64> {
    state.engine.login_recording()
}

#[tauri::command]
pub(crate) async fn login_recording_stop(
    state: State<'_, AppState>,
) -> CmdResult<chosproxy_app::LoginRecording> {
    state.engine.login_recording_stop().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn agent_continue(
    state: State<'_, AppState>,
    task: i64,
    message: String,
) -> CmdResult<chosproxy_storage::TaskRow> {
    state
        .engine
        .agent_continue(task, message)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn ai_test(
    state: State<'_, AppState>,
    config: chosproxy_core::config::AiConfig,
) -> CmdResult<chosproxy_app::AiAnswer> {
    state.engine.ai_test(config).await.map_err(err)
}

#[tauri::command]
pub(crate) fn agent_config(state: State<'_, AppState>) -> chosproxy_core::config::AgentConfig {
    state.engine.agent_config()
}

#[tauri::command]
pub(crate) fn save_agent_config(
    state: State<'_, AppState>,
    config: chosproxy_core::config::AgentConfig,
) -> CmdResult<chosproxy_core::config::AgentConfig> {
    state.engine.save_agent_config(config).map_err(err)
}

#[tauri::command]
pub(crate) fn agent_tools(state: State<'_, AppState>) -> Vec<chosproxy_app::AgentToolInfo> {
    state.engine.agent_tools()
}

#[tauri::command]
pub(crate) async fn agent_start(
    state: State<'_, AppState>,
    start: chosproxy_app::AgentStart,
) -> CmdResult<chosproxy_storage::TaskRow> {
    state.engine.agent_start(start).await.map_err(err)
}

#[tauri::command]
pub(crate) fn agent_state(state: State<'_, AppState>, task: i64) -> chosproxy_app::AgentState {
    state.engine.agent_state(task)
}

#[tauri::command]
pub(crate) fn agent_approvals(state: State<'_, AppState>) -> Vec<chosproxy_app::AgentApproval> {
    state.engine.agent_approvals()
}

#[tauri::command]
pub(crate) fn agent_decide(
    state: State<'_, AppState>,
    approval: i64,
    approve: bool,
    note: String,
) -> CmdResult<()> {
    state
        .engine
        .agent_decide(approval, approve, note)
        .map_err(err)
}

#[tauri::command]
pub(crate) fn agent_message(state: State<'_, AppState>, task: i64, text: String) -> CmdResult<()> {
    state.engine.agent_message(task, text).map_err(err)
}

#[tauri::command]
pub(crate) async fn ai_activity(
    state: State<'_, AppState>,
    task: Option<i64>,
    after: i64,
    limit: u32,
) -> CmdResult<Vec<chosproxy_storage::AiActivity>> {
    state
        .engine
        .ai_activity(task, after, limit)
        .await
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn temp_status(
    state: State<'_, AppState>,
) -> CmdResult<chosproxy_app::TempStatus> {
    state.engine.temp_status().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn temp_cleanup(
    state: State<'_, AppState>,
) -> CmdResult<chosproxy_app::TempCleanup> {
    state.engine.temp_cleanup().await.map_err(err)
}

#[tauri::command]
pub(crate) async fn temp_remove(
    state: State<'_, AppState>,
    names: Vec<String>,
) -> CmdResult<chosproxy_app::TempCleanup> {
    state.engine.temp_remove(names).await.map_err(err)
}

#[tauri::command]
pub(crate) async fn performance(
    state: State<'_, AppState>,
) -> CmdResult<chosproxy_app::PerformanceSnapshot> {
    state.engine.performance().await.map_err(err)
}
