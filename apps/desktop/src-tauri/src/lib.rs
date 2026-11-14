//! Tauri shell for ChosProxy. Commands are thin delegations to
//! [`chosproxy_app::Engine`]; this crate holds no business logic.

mod automation;
mod commands;
mod events;

use chosproxy_app::Engine;
use chosproxy_app::logging::{self, LogHandle};
use chosproxy_core::paths::AppPaths;
use tauri::Manager;

/// Shared state managed by Tauri.
pub(crate) struct AppState {
    pub(crate) engine: Engine,
    pub(crate) log: Option<LogHandle>,
    pub(crate) automation: automation::Automation,
}

/// Starts the desktop application.
pub fn run() {
    let Some(paths) = AppPaths::resolve() else {
        eprintln!("cannot determine configuration directories (is HOME set?)");
        std::process::exit(1);
    };
    let engine = match Engine::new(paths.clone()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("cannot start: {e}");
            std::process::exit(1);
        }
    };
    let log = match logging::init(&paths.log_dir, engine.settings().logging.level) {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("logging disabled: {e}");
            None
        }
    };
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            engine: engine.clone(),
            log,
            automation: automation::Automation::default(),
        })
        .setup(move |app| {
            events::forward(app.handle().clone(), engine.subscribe());
            if engine.api_config().enabled {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    let status = state.automation.restart(&state.engine).await;
                    if let Some(e) = status.error {
                        tracing::warn!(error = %e, "automation API not started");
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::get_settings,
            commands::save_settings,
            commands::get_proxy_config,
            commands::save_proxy_config,
            commands::list_listeners,
            commands::start_listener,
            commands::stop_listener,
            commands::restart_listener,
            commands::current_project,
            commands::new_temporary_project,
            commands::create_project,
            commands::open_project,
            commands::close_project,
            commands::history_page,
            commands::exchange_detail,
            commands::dashboard_stats,
            commands::ca_status,
            commands::export_ca,
            commands::regenerate_ca,
            commands::import_ca,
            commands::list_browsers,
            commands::browser_status,
            commands::launch_browser,
            commands::close_browser,
            commands::intercept_state,
            commands::set_intercept,
            commands::held_message,
            commands::unpack_held,
            commands::forward_held,
            commands::drop_held,
            commands::forward_all_held,
            commands::set_listener_auth,
            commands::event_log,
            commands::clear_event_log,
            commands::decode,
            commands::smart_decode,
            commands::organizer_add,
            commands::organizer_list,
            commands::organizer_detail,
            commands::organizer_update,
            commands::organizer_delete,
            commands::organizer_export,
            commands::organizer_import,
            commands::is_sealed_file,
            commands::ws_connections,
            commands::ws_messages,
            commands::ws_message,
            commands::ws_export,
            commands::save_project_as,
            commands::change_project_password,
            commands::export_project,
            commands::project_settings,
            commands::save_backup_settings,
            commands::list_backups,
            commands::backup_now,
            commands::restore_backup,
            commands::scope_override,
            commands::set_scope_override,
            commands::project_notes,
            commands::save_project_notes,
            commands::audit_log,
            commands::security_overview,
            commands::export_settings,
            commands::preview_settings_import,
            commands::apply_settings_import,
            commands::findings,
            commands::finding_detail,
            commands::finding_create,
            commands::finding_add_evidence,
            commands::finding_remove_evidence,
            commands::finding_update,
            commands::finding_delete,
            commands::report_preview,
            commands::export_report,
            commands::get_scope,
            commands::save_scope,
            commands::check_scope,
            commands::add_url_to_scope,
            commands::site_map,
            commands::set_site_mark,
            commands::annotate_exchange,
            commands::search,
            commands::repeater_tabs,
            commands::repeater_create,
            commands::repeater_from_exchange,
            commands::repeater_update,
            commands::repeater_duplicate,
            commands::repeater_move,
            commands::repeater_close,
            commands::repeater_send,
            commands::repeater_cancel,
            commands::repeater_history,
            commands::repeater_snapshot,
            commands::repeater_redirect,
            commands::repeater_groups,
            commands::repeater_group_create,
            commands::repeater_group_update,
            commands::repeater_group_delete,
            commands::repeater_set_group,
            commands::repeater_set_color,
            commands::repeater_send_group,
            commands::repeater_cancel_group,
            commands::cookie_jar,
            commands::cookie_jar_set_value,
            commands::cookie_jar_delete,
            commands::cookie_jar_clear,
            commands::passive_status,
            commands::passive_checks,
            commands::active_checks,
            commands::passive_reanalyze,
            commands::tasks,
            commands::task_detail,
            commands::task_pause,
            commands::task_resume,
            commands::task_cancel,
            commands::task_set_pool,
            commands::crawl_graph,
            commands::task_delete,
            commands::crawl_defaults,
            commands::crawl_start,
            commands::fuzz_defaults,
            commands::fuzz_builtin_lists,
            commands::fuzz_auto_mark,
            commands::fuzz_preview,
            commands::fuzz_start,
            commands::fuzz_results,
            commands::fuzz_set_color,
            commands::fuzz_export,
            commands::sequencer_defaults,
            commands::sequencer_prepare,
            commands::sequencer_start,
            commands::sequencer_analysis,
            commands::sequencer_analyze,
            commands::sequencer_tokens,
            commands::plugins,
            commands::plugins_dir,
            commands::plugin_set,
            commands::plugin_logs,
            commands::plugin_actions,
            commands::plugins_trust_settings,
            commands::plugin_trust,
            commands::plugins_require_signed,
            commands::plugins_install_from,
            commands::plugin_run_action,
            commands::plugins_install_example,
            commands::session_rules,
            commands::save_session_rules,
            commands::credentials,
            commands::credential_set,
            commands::credential_delete,
            commands::keyring_status,
            commands::keyring_unlock,
            commands::keyring_lock,
            commands::keyring_set_protection,
            commands::scripts,
            commands::save_scripts,
            commands::export_scripts,
            commands::import_scripts,
            commands::script_test,
            commands::script_action,
            commands::script_column,
            commands::history_page_scripted,
            commands::history_page_conditions,
            commands::site_map_conditions,
            commands::network_config,
            commands::save_network_config,
            commands::oast_config,
            commands::oast_save_config,
            commands::oast_status,
            commands::oast_test_server,
            commands::oast_new_payload,
            commands::oast_payloads,
            commands::oast_interactions,
            commands::oast_interaction,
            commands::dom_launch_browser,
            commands::dom_status,
            commands::dom_navigate,
            commands::dom_events,
            commands::dom_clear,
            commands::discovery_defaults,
            commands::discovery_start,
            commands::browser_crawl_start,
            commands::live_task_start,
            commands::scan_summary,
            commands::resource_pools,
            commands::save_resource_pools,
            commands::scan_configs,
            commands::save_scan_config,
            commands::delete_scan_config,
            commands::export_scan_configs,
            commands::tool_configs,
            commands::save_tool_config,
            commands::delete_tool_config,
            commands::export_tool_configs,
            commands::import_tool_configs,
            commands::import_scan_configs,
            commands::scan_start,
            commands::api_import,
            commands::api_definitions,
            commands::api_definition,
            commands::api_definition_delete,
            commands::api_request,
            commands::api_send_start,
            commands::graphql_introspect,
            commands::graphql_import_schema,
            commands::graphql_schema,
            commands::graphql_build,
            commands::graphql_request,
            commands::site_node_details,
            commands::api_status,
            commands::save_api_settings,
            commands::regenerate_api_token,
            commands::ai_config,
            commands::save_ai_config,
            commands::ai_preview,
            commands::ai_run,
            commands::ai_test,
            commands::ai_run_stream,
            commands::ai_cancel,
            commands::login_recording_start,
            commands::login_recording,
            commands::login_recording_stop,
            commands::agent_continue,
            commands::agent_config,
            commands::save_agent_config,
            commands::agent_tools,
            commands::agent_start,
            commands::agent_state,
            commands::agent_approvals,
            commands::agent_decide,
            commands::agent_message,
            commands::ai_activity,
            commands::temp_status,
            commands::temp_cleanup,
            commands::temp_remove,
            commands::performance,
            commands::suppressions,
            commands::suppression_add,
            commands::suppression_delete,
            commands::discovery_results,
            commands::target_analysis,
            commands::snapshot_save,
            commands::snapshots,
            commands::snapshot_delete,
            commands::snapshot_compare,
            commands::schedules,
            commands::schedule_create,
            commands::schedule_set_enabled,
            commands::schedule_delete,
            commands::repeater_set_note,
            commands::repeater_set_redirect_policy,
            commands::open_project_read_only,
            commands::recovery_status,
            commands::recovery_restore,
            commands::recovery_open_read_only,
            commands::recovery_discard,
        ])
        .build(tauri::generate_context!());
    let app = match app {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(error = %e, "failed to build the application");
            eprintln!("failed to start: {e}");
            std::process::exit(1);
        }
    };
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            let engine = handle.state::<AppState>().engine.clone();
            // Flush captures and delete temporary projects before exiting.
            tauri::async_runtime::block_on(engine.shutdown());
        }
    });
}
