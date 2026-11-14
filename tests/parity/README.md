# Parity test suite

Each row of [docs/BURP_PARITY_MATRIX.md](../../docs/BURP_PARITY_MATRIX.md)
has an ID (`PROXY-001`, `REPEATER-001`, …). A row may only become 🟢 when it
links to its implementation, documentation and at least one test here or in
the crate that implements it.

Convention: name tests after the ID, e.g.
`fn proxy_001_listener_relays_and_captures()`, so
`cargo test proxy_001` selects every test for that capability.

Current mapping:

| ID | Tests |
|---|---|
| PROXY-001 | `crates/proxy/tests/relay.rs` (`relays_and_captures_a_get`, `keeps_client_connection_alive_across_requests`, `stopping_a_listener_closes_it`) |
| PROXY-002 | `crates/storage/src/db.rs` (`insert_query_and_load`), `crates/app/tests/engine.rs` |
| PROXY-003 | `crates/proxy/tests/https.rs` (all), `crates/tls/src/leaf.rs` (handshake tests), `crates/app/tests/engine.rs` (`https_is_intercepted_and_ca_is_managed`) |
| PROXY-004 | `crates/app/src/ca_manager.rs` tests, `crates/proxy/tests/https.rs` (`ca_certificate_is_downloadable_from_the_proxy`) |
| PROXY-006 | `crates/tls/src/leaf.rs`, `crates/tls/src/ca.rs` tests |
| PROXY-014 | `crates/proxy/tests/relay.rs` (`connect_tunnels_bytes_opaquely`), `crates/proxy/tests/https.rs` (`passthrough_hosts_and_disabled_interception_are_not_decrypted`) |
| PROXY-007 | `crates/proxy/tests/intercept.rs`, `crates/app/tests/engine.rs` (`intercepted_request_is_edited_through_the_engine`), `apps/desktop/src/state/intercept.test.ts` |
| PROXY-008 | `crates/proxy/tests/intercept.rs` (`responses_can_be_paused_edited_and_kept_alive`, `forward_and_intercept_response_pauses_the_response_too`) |
| PROXY-009 | `crates/core/src/config/proxy.rs` (`intercept_rules`), `crates/proxy/tests/intercept.rs` (`rules_let_static_assets_through`) |
| EDITOR-001 | `crates/http/src/edit.rs`, `apps/desktop/src/lib/{bytes,pretty}.test.ts` |
| BROWSER-001 | `crates/app/src/browser.rs`, `crates/app/src/browser_session.rs` tests |
| PROXY-018 | `crates/proxy/tests/relay.rs` (`upstream_failures_are_reported`, `reconnects_when_upstream_closed_idle_connection`) |
| PROXY-022 | `crates/proxy/tests/relay.rs` (`preserves_header_case_order_and_spacing_upstream`, `cl_te_conflict_is_flagged`) |
| PROJECT-001 | `crates/app/tests/engine.rs` (`temporary_project_captures_and_cleans_up`, `persistent_project_survives_reopen`) |
| SETTINGS-001 | `crates/core/src/config/*` tests |
