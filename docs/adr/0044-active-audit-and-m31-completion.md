# ADR-0044: Active Audit Framework (M13) and M31 Completion (Crash Recovery, Encrypted Sharing, Security Center)

* Status: Accepted
* Date: 2026-11-14

## Context

After M33, four planned areas remained:
1. **M31 A — Crash recovery (`PROJECT-004`, spec §118)**: recovering an open project after an unexpected process exit without interfering with live sibling instances, re-sending traffic automatically, or corrupting encrypted/temporary databases.
2. **M31 C — Encrypted sharing (`ORGANIZER-003`)**: exposing the password-sealed Organizer and settings export/import format in the Tauri IPC and desktop UI with consistent nested collection semantics and atomic file writes.
3. **M31 F — Security Center (§120) and project audit log coverage (§122, `PLATFORM-002`)**: a local-only posture overview across ChosProxy's security-relevant settings and complete, filterable project audit log coverage.
4. **M13 — Active Audit Framework (`SCANNER-002`, `SCANNER-004`, `SCANNER-005`, `SCANNER-011`, `SCANNER-013`, `SCANNER-014`, `OAST-001`, spec §23)**: an opt-in, scope-enforced, rate- and budget-bounded active scanner reusing the existing insertion-point engine (`crates/scanner/src/insertion.rs`), Request Engine, Resource Pools, Session Rules, and findings store.

## Decisions

### 1. Crash recovery and read-only projects (M31 A)
* **Per-instance session markers**: each running `Engine` writes `sessions/<session_id>.json` (`0600`) atomically on project open, close, Save As, and task start/stop. Markers contain only non-secret metadata (`session_id`, `pid`, `process_start_time`, project path/name/kind, `encrypted` flag, and running task summaries).
* **Sibling-instance safety**: `recovery_status()` inspects `sessions/` excluding the current instance's `session_id` and verifies process liveness via `sysinfo` (`pid` + `process_start_time`, with a 2 s tolerance). Markers belonging to live processes are left untouched.
* **Safe restore and read-only mode**:
  * `recovery_restore` runs `PRAGMA quick_check` before opening, prompts for the password on encrypted projects (leaving the marker intact on a wrong password), marks interrupted traffic-sending tasks (`crawl`, `scan`, `fuzz`, `sequencer`, `discovery`, `browserCrawl`, `apiSend`, `agent`) as `TaskStatus::Cancelled`, and resumes only passive live tasks (`liveAudit`, `liveCrawl`).
  * `open_project_read_only` / `recovery_open_read_only` opens SQLite with `SQLITE_OPEN_READ_ONLY` and sets `ProjectInfo.read_only = true`. `Engine::ensure_writable()` blocks proxy listeners, tool sends, tasks, and database mutations while allowing full read inspection and `Save as…`.
  * Recoverable temporary projects are protected from automatic cleanup in `temp_ops.rs` until the user restores or discards recovery.

### 2. Encrypted sharing UI, nested collections, and atomic writes (M31 C)
* **Nested collection prefix matching**: exporting collection `Parent` exports items in `Parent` and any `Parent/...` sub-collection consistently across storage, engine, and UI.
* **Format inspection before password prompt**: `inspect_export_file` checks the `CHOSSEAL` header before the UI prompts for a password.
* **Atomic `0600` writes**: exports write to a temporary sibling file and `rename` into place so failed exports never truncate an existing file.

### 3. Security Center (§120) and project audit log (§122) (M31 F)
* **Local-only posture overview**: `Engine::security_overview()` evaluates 13 checks (`ca_certificate`, `proxy_listeners`, `automation_api`, `plugins_permissions`, `plugins_signing`, `chos_ai`, `chos_agent`, `upstream_proxies`, `oast`, `tls_verification`, `project_encryption`, `temp_files`, `keyring_protection`) from live configuration and filesystem metadata without network I/O and without exposing keys, passwords, or tokens.
* **Complete audit log & filtering**: `audit_log_query` supports `action_prefix`, `search`, `from_ms`, `to_ms`, `limit`, and `offset`, exposed in `ProjectView.tsx` (`AuditCard`).

### 4. Active Audit Framework (M13)
* **Pure probe planning vs network execution**: `crates/scanner/src/active.rs` defines `ActiveCheckInfo`, `ACTIVE_CHECKS`, `plan_point_probes`, `plan_request_probes`, `evaluate_point_probes`, and `evaluate_request_probes` as pure functions over `InsertionPoint` and `Message`. Network I/O lives in `crates/app/src/engine/scan_ops.rs` (`run_active_audit`), which sends probes through `session_send("crawler", Source::Scanner, ...)` so Resource Pools, Session Rules, upstream proxies, and history logging apply uniformly.
* **Strict launch preconditions**: active audit is off by default (`ScanConfig.active = false`). Launching or scheduling an active scan requires:
  1. `active_authorized == true` (stripped to `false` whenever a scan configuration is saved or imported so saved templates never bypass confirmation);
  2. a non-empty project scope;
  3. every start URL inside the project scope; and
  4. validated rate (`1..=100` req/s), concurrency (`1..=32`), per-point (`1..=50`), and total (`1..=10,000`) probe budgets.
* **Per-probe scope revalidation and safe method defaults**: non-idempotent methods (`POST`, `PUT`, `PATCH`, `DELETE`) are skipped unless `active_allow_unsafe_methods` is explicitly enabled. Scope is re-checked before every probe so removing a host from scope mid-scan stops further probes to that host immediately.
* **Honest confidence and non-destructive checks**:
  * `active-reflected-xss` uses innocuous markers (`chosprobe7x9k` and `chosprobe7x9k"'><chosx>`) and reports `Confidence::Tentative` because response reflection without browser execution does not prove script execution.
  * `active-sqli` uses controlled boolean comparisons (`OR 1=1` vs `AND 1=2`) paired with a broken-quote negative syntax control and corroborated DBMS error signatures (`Confidence::Firm`), suppressing dynamic pages and generic 500s without extracting data.
  * `active-open-redirect` inspects `3xx` `Location` headers for external probe hosts (`Confidence::Certain`) without following redirects.
  * `active-cors` tests arbitrary and `null` `Origin` reflection with `Access-Control-Allow-Credentials: true` (`Confidence::Certain`).
  * `active-oast` injects correlated ChosOAST URLs when OAST is enabled and records `Confidence::Certain` only when a matching interaction arrives for that project's token.
* **Dual evidence (`SCANNER-011`)**: `active_issue_pair` attaches both the baseline exchange and the proving probe exchange to the finding's evidence list.
* **Custom active check scripts (`SCANNER-013`) and API endpoint active audit (`SCANNER-014`)**: `ScriptKind::Check` scripts may define `fn active_probes(value)` (capped at 16 probes per point) and `fn active_check(baseline, probe, payload)` within the same Rhai sandbox and scan budgets. The API workspace adds **Audit endpoints…** to open the scan launcher pre-filled with the API definition's target URLs.

## Consequences

* `PROJECT-004`, `ORGANIZER-003`, `PLATFORM-002`, `SCANNER-005`, `SCANNER-011`, `SCANNER-013`, and `SCANNER-014` reach 🟢 in `docs/BURP_PARITY_MATRIX.md`.
* `SCANNER-002` advances from 🔵 to 🟡 (honestly reflecting that the active audit engine and initial 5-check catalog + custom scripts are complete, while broader multi-variant active families and browser-verified active XSS remain outside the initial catalog).
* Parity matrix summary stands at **100 🟢 / 30 🟡 / 2 🔵 / 2 ➖** across 134 rows. Full parity is still not claimed.
