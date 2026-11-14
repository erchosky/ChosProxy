# Where the project stands (handoff notes)

Read this first when you pick the work up in a new session. Keep it up to
date: change it, and commit it, whenever a milestone step lands.

_Last updated: 2026-11-14. M31 (including steps A, C and F) and M13 (Active Audit Framework) are done on `main`._

## Snapshot

- **Product:** ChosProxy, a web security workbench in the spirit of Burp
  Suite. It has a Rust workspace (`crates/*`) and a Tauri 2 + React +
  TypeScript desktop app (`apps/desktop`).
- **Done, on `main`:** M0–M33 (including M31 A crash recovery, M31 C
  encrypted sharing IPC/UI, M31 F Security Center and audit coverage, and
  M13 Active Audit Framework; ADR-0044). M17 is tagged `v1.0.0`.
- **Now:** all planned milestones M0–M33 are complete on `main`.
  **Parity is not declared** (remaining partial/planned items are listed below).
- **Final release check (2026-11-14):** all checks green (822 Rust tests,
  224 vitest, clippy, fmt, tsc, eslint, bindings, matrix, bundle budget);
  release `.app` and DMG built and smoke-tested in the real app (temporary
  project, HTTP/HTTPS through the proxy with the lab, history, Repeater,
  Site map, Fuzzer, Settings/Security Center, clean quit with no WARN/ERROR
  logs). Packaging scripts fixed for iCloud xattrs. Package with:
  `pnpm --filter @chosproxy/desktop tauri build --bundles app && scripts/make-macos-dmg.sh && scripts/make-backup.sh mac`.
- **Branches:** none. Since 2026-11-12 the user wants everything on
  `main`: no feature branches, no stashes; commit each step to `main`.
- **Parity matrix:** after M31 (A, C, F) and M13, 100 rows complete,
  30 partial, 2 planned (needs decision), 2 not applicable
  ([docs/BURP_PARITY_MATRIX.md](docs/BURP_PARITY_MATRIX.md)).

## Disk space and iCloud (read this if git or vitest behave oddly)

The repo lives in `~/Documents`, which iCloud syncs. When the disk is
nearly full, macOS evicts files to iCloud ("dataless"), and then:
- git fails with `mmap failed: Operation canceled`, "packfile far too
  short" or hangs in `git add`;
- vitest fails with "Failed to start forks worker" or "Class extends value
  undefined";
- a killed git command can leave `.git/index` empty ("index file smaller
  than expected").

On 2026-11-12 the cause was `target/` at 150 GB (the disk had 7.7 GB
free). `cargo clean` freed 227 GB. **Check `df -h ~` and `du -sh target`
now and then, and run `cargo clean` when `target` grows past ~50 GB.**

Fixes:
- download evicted files: `find .git crates apps -flags +dataless -type f -print0 | xargs -0 -P 32 -n 4 cat > /dev/null`;
- `node_modules` damaged: `rm -rf node_modules apps/desktop/node_modules && CI=true pnpm install --frozen-lockfile`;
- empty `.git/index`: move it away and run `git reset -q` (the working
  tree is kept); then `git fsck --no-dangling`;
- iCloud can also leave duplicates named `name 2.ext` next to files
  (seen 2026-11-13): untracked, so `git status` shows them; compare with
  the original (`cmp`) and move them out of the repo.

## How the work is done (the loop)

1. Plan the milestone in this file as a checklist and commit it to `main`.
2. Implement it step by step on `main`; tick and commit each step here.
3. Finish each milestone with docs and checks, then commit:
   - update ROADMAP (row ✅ plus a "Milestone N (done)" section with an
     explicit "Deferred" list naming target milestones), CHANGELOG, README
     (feature table), THREAT_MODEL rows, an ADR in `docs/adr/` (and its
     index), and the parity matrix rows;
   - regenerate the matrix summary table by counting the rows.
4. Tell the user and ask before starting the next milestone.
   - Do **not** push or tag unless the user asks. There is no git remote.
   - Commit messages end with
     `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>` (the model
     that does the work).

## Checks before every commit

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -q -- -D warnings
cargo test --workspace -q          # 822 tests passing (+3 ignored perf benches)
cd apps/desktop
npx tsc -b                         # NOT `tsc --noEmit -p .` (misses tests)
npx eslint src                     # 2 known warnings; `eslint .` can hang
npx prettier --check src
npx vitest run                     # 224 tests across 71 files (M13 + M31)
cd ../..
node scripts/check-bindings.mjs    # needs committed ts-rs bindings
node scripts/matrix-summary.mjs --check   # the matrix table matches its rows
# after UI changes that touch the shell:
(cd apps/desktop && npx vite build) && node scripts/check-bundle.mjs
```

- `cargo test` regenerates the TypeScript bindings in
  `apps/desktop/src/bindings` (ts-rs). Run it after changing Rust types,
  then commit them.
- Markdown is not part of the Prettier check.
- Run Prettier **from `apps/desktop`** (`npx prettier --write src`). Run
  from the repo root, it ignores the app's `.prettierignore` and reformats
  the generated bindings, which then fails `check-bindings`.

## Engineering and product requirements

- Only test systems you own or are authorized to test.
- No stealth, evasion, persistence, credential stuffing, password spraying,
  destructive exploitation or DoS. No credential lists are shipped.
- M13 delivered active audit reusing M25 passive scanner infrastructure
  (`crates/scanner`, `engine/scan_ops.rs`, `findings`, `pool_ops.rs`,
  `session_ops.rs`).
- "Simulate manual testing traffic" was deliberately not implemented (➖,
  ADR-0028).
- Nothing is granted to plugins automatically.
- Never execute untrusted response content in the privileged Tauri context.
- No telemetry.

## Architecture pointers

- **Rust 2024:** clippy with `-D warnings`, `unwrap_used` denied, no
  `unsafe`, minimum Rust 1.89.
- **Config sections:** one TOML file each (`crates/core/src/config/*`,
  `ConfigSection`):
  - `proxy.toml`, `network.toml` (M21), `scripts.toml` (M22),
    `settings.toml`, `oast.toml`, `plugins.toml`, scope.
- **Request Engine** (`crates/request`): every outgoing connection goes
  through `Upstream::connect` / `tunnel` / `connect_h2`. Upstream proxies,
  host overrides and timeouts come from process-wide `network()`. HTTP/2 is
  in `h2.rs`.
- **Proxy** (`crates/proxy`):
  - `conn.rs`: listener sessions, invisible mode;
  - `relay.rs`: the byte-exact HTTP/1 relay;
  - `h2_relay.rs`: HTTP/2 to clients;
  - `match_replace.rs`, `websocket.rs`.
- **HTTP/2 messages** use the text form `GET / HTTP/2` / `HTTP/2 200`
  (ADR-0030/0031).
- **Scripts:** `crates/script` (Rhai sandbox), engine integration in
  `crates/app/src/engine/script_ops.rs`, UI in Tools → Scripts. Guide in
  docs/SCRIPTS.md.
- **SQLite migrations** are at v29 (`crates/storage/migrations/0029_*`).
- **Tauri commands:** `apps/desktop/src-tauri/src/commands.rs`, registered
  in `lib.rs`, wrapped in `apps/desktop/src/services/ipc.ts`.

## Testing in the app (macOS)

- Build:
  ```bash
  cargo build -p chosproxy-desktop -p chosproxy-lab
  cp target/debug/chosproxy target/debug/bundle/macos/ChosProxy.app/Contents/MacOS/chosproxy
  ```
- Run:
  - the lab with `target/debug/chosproxy-lab` (ports 9090 and 9443);
  - vite with `cd apps/desktop && npx vite --port 1420`;
  - the app with
    `open --env CHOSPROXY_HOME=<scratch dir> target/debug/bundle/macos/ChosProxy.app`.
- Send traffic with `curl -x http://127.0.0.1:8080 …` after opening a
  (temporary) project.
- In the last sessions macOS returned no window capture, so the UI was
  verified by tests only. The listener starts only with an open project.

## Selected next work (user request, 2026-11-14) — Checklist

- [x] **M31 A — Crash recovery (`PROJECT-004`, spec §118)**:
  - [x] Versioned session marker (`sessions/<session_id>.json` or `session.json` with instance isolation) in `data_dir`, updated atomically (`0600`) on project open/close/save-as and task start/stop, storing only metadata (never keys, passwords, request bodies or tokens), and removed cleanly on exit/shutdown for the current session only.
  - [x] Process liveness detection (PID + process start time via `sysinfo`) distinguishing active sibling instances, reused PIDs, corrupt markers and missing/moved project files without stealing another live session.
  - [x] Engine operations: `recovery_status()`, `recovery_restore(password)`, `open_project_read_only(path, password)` / `recovery_open_read_only(password)` and `recovery_discard(delete_orphan_temp)`.
  - [x] Restore runs `quick_check` before opening, prompts for password on encrypted projects (wrong password or corrupt DB preserves marker and any open project), marks interrupted traffic-sending tasks as cancelled (`TaskStatus::Cancelled` with interruption reason in log/error), and resumes passive live tasks as today.
  - [x] Read-only enforcement across the engine (`ProjectInfo.read_only`, `ensure_writable` on listeners, tool sends, tasks, settings overrides, notes, findings, repeater, organizer, etc.), visible read-only banner in UI, and startup recovery dialog (`Restore`, `Open read-only`, `Discard recovery`, cancel).
  - [x] Protect recoverable temp projects in `temp_ops.rs` (`temp_status`, `temp_cleanup`, `temp_remove`); optional confirmed removal of orphaned temporary project on discard.
  - [x] Integration tests (`crates/app/tests/recovery.rs`) and UI tests covering stale/corrupt markers, live sibling instance, interrupted tasks, read-only enforcement, missing paths, encrypted recovery, cancellation and temp cleanup coordination.
- [x] **M31 C — Encrypted exports (`ORGANIZER-003`)**:
  - [x] Define and apply consistent nested collection semantics (`collection` prefix match vs exact match: exporting a parent collection includes its `parent/...` descendants consistently in engine, UI and tests).
  - [x] Atomic file writes (`0600`) for Organizer and settings exports so failed exports never destroy an existing file; format detection (`is_sealed` / `inspect_export_file`) before prompting for a password.
  - [x] Tauri commands, registrations, `ts-rs` bindings and `services/ipc.ts` wrappers for `organizer_export`, `organizer_import`, `export_settings_with`, `preview_settings_import_with`, `apply_settings_import_with` and sealed-file inspection.
  - [x] UI in `OrganizerView.tsx` (Export collection / all, plain or password-sealed; Import with format detection, password prompt, retry/cancel) and `SettingsTransferCard.tsx` (optional password on export, password prompt on sealed import before preview diff).
  - [x] Engine integration tests (`crates/app/tests/encrypted_share.rs`) and UI tests (`OrganizerView` / `SettingsTransferCard`) covering round-trip bytes/metadata/nested collections, wrong password, tampered header/ciphertext, unknown version, truncated file, invalid item rollback, plain + sealed settings, cancellation and write failures.
- [x] **M31 F — Security Center (§120) and audit coverage (§122, `PLATFORM-002`)**:
  - [x] Engine `security_overview()` returning typed checks (`ok` / `warn` / `info` / `unavailable`, summary counts, navigation targets) from real engine/config state: CA status & key path (never key material), listeners bound beyond loopback & auth, Automation API, plugins & permissions, signed-only policy, ChosAI mode/provider, ChosAgent `Allow` permissions, upstream proxies, OAST mode & external connections, TLS verification exceptions, project encryption & read-only state, temporary files & cleanup.
  - [x] Complete §122 project audit log events without duplicating existing ones: plugin permissions granted/revoked, traffic tasks started/finished/cancelled/failed (crawl, scan, fuzz, sequencer, discovery, browser crawl, API send), ChosAgent approvals/rejections, crash recovery actions, project encryption/decryption/rekey, exports and scope/finding changes; safe handling when no writable project is open.
  - [x] Expand `audit_log` query/filtering (action prefix/category, date range, text search, stable order, visible limit/pagination) in storage, engine, Tauri, IPC and `ProjectView.tsx` (`AuditCard`).
  - [x] Add `Security & Privacy` view in Settings (and palette/navigation links to each setting, working with or without an open project, strictly local).
  - [x] Integration tests (`crates/app/tests/security_center.rs`) and UI tests covering every check, every audit event family, no secret leakage, read-only safety and audit log filters/reopen persistence.
- [x] **M13 — Active Audit Framework (`SCANNER-002`, `SCANNER-004`, `SCANNER-005`, `SCANNER-011`, `SCANNER-013`, `SCANNER-014`, `OAST-001`, spec §23)**:
  - [x] Typed active check contract in `crates/scanner`: stable ID, issue metadata, compatible insertion point kinds, preconditions, request budget, pure probe generation and deterministic evidence evaluation separated from network I/O.
  - [x] Initial non-destructive check catalog: reflected input & context analysis for XSS (innocuous markers, tentative vs confirmed semantics), SQL injection via controlled boolean comparisons and corroborated SQL error signals (with dynamic-page and negative controls, no data extraction), open redirect via `Location` header inspection (without following out-of-scope redirects), CORS misconfiguration active probing with baseline comparison, and optional ChosOAST interaction checks when OAST is configured and enabled.
  - [x] Active check support in `crates/script` / ChosChecks (`SCANNER-013`) and active audit of API workspace endpoints (`SCANNER-014`) within the same bounded, scoped engine.
  - [x] Engine active audit phase (`engine/scan_ops.rs`): off by default, requires explicit target, defined scope, user authorization confirmation and rate/request budgets (validated in engine, scheduler and API/IPC); shares Resource Pools and Session Rules via `session_send`; revalidates scope on every request and redirect; updates framing/`Content-Length` per `PointEncoding`; supports pause/resume/cancel immediately releasing slots; records traffic as `Source::Scanner` with check/point notes and dashboard progress.
  - [x] UI updates in `ScanDialog.tsx`, `CheckPicker.tsx`, `ScanDashboard.tsx` and API workspace for selecting active checks, insertion point kinds, budget preview and explicit authorization gate.
  - [x] Deterministic local lab endpoints and integration tests (`crates/app/tests/active_scan.rs`) + UI tests: vulnerable vs fixed, dynamic responses, generic 500s, escaped contexts, SQL negative controls, out-of-scope redirects, live scope removal, budget limits, cancellation, session rules, dedup/suppressions and correlated vs foreign OAST interactions.
- [x] **Validation and documentation**:
  - [x] Update `STATUS.md`, `ROADMAP.md`, `CHANGELOG.md`, `README.md`, `THREAT_MODEL.md`, user docs (`docs/PROJECT_FILES.md`, `docs/SCRIPTS.md`), ADR-0044 (and index), and `docs/BURP_PARITY_MATRIX.md` + `node scripts/matrix-summary.mjs`.
  - [x] Run full verification suite (`cargo fmt`, `cargo clippy`, `cargo test`, `tsc -b`, `eslint`, `prettier`, `vitest`, `check-bindings.mjs`, `matrix-summary.mjs --check`, `vite build` + `check-bundle.mjs`).

## Other decisions still open

- Runtime instrumentation (INSTR-001); extension editor tabs (EDITOR-007).
- ACME for a self-hosted OAST server; model and OAST providers through
  plugins; the plugin OS sandbox.
- Raw and streaming HTTP/2 frames in Repeater (its own milestone).
- GraphQL-specific checks; other payload generators; replaying recorded
  browser interactions; script data-flow analysis.
- Secret Service on Linux for the keyring key (D-Bus in the Kali build);
  Kerberos; NTLM to an upstream proxy.
- Small ones left: PDF reports (a new dependency, or print the HTML
  report); dropping (not only not storing) out-of-scope proxy traffic was
  decided against in ADR-0043.
- Whether to declare parity with those gaps written down, and whether to
  push or tag anything (there is no git remote).

## Done: M33 — Small gaps (2026-11-14, on main)

- [x] Repeater groups collapse (schema v28, `repeater_groups.collapsed`).
- [x] Target → Scope: `ScopeConfig.skip_out_of_scope_proxy`; the sink
      (`crates/app/src/sink.rs`, `StoreFilter`) skips out-of-scope
      exchanges, WebSockets and cookies; test
      `out_of_scope_proxy_traffic_can_be_left_unstored`.
- [x] Fuzzer: `FuzzConfig.stop` (`StopConditions`, TS
      `FuzzStopConditions`), CSV export (`fuzz_export`, formula-guarded),
      result colours (schema v29), hideable columns (browser storage);
      test `stop_conditions_export_and_highlights`.
- [x] Event log in `logs/event-log.jsonl` (`EventLog::persistent`).
- [x] `Engine::with_launch_pool` + `pool` on the start commands +
      `LaunchPoolSelect`; test
      `a_pool_chosen_at_launch_carries_the_first_requests`.
- [x] `CodeView.test.tsx` (editor search).
- [x] Docs: ADR-0043, ROADMAP row 33, CHANGELOG, THREAT_MODEL T84–T85,
      matrix 94 🟢 / 34 🟡 / 4 🔵 / 2 ➖.
- Not implemented during M33: M13 and M31 steps A, C, F; selected next on 2026-11-14.

## Done: M32 — Parity audit (2026-11-14, on main)

Plan and result (one commit):
- [x] Check every row of docs/BURP_PARITY_MATRIX.md against the code and
      the tests: every test reference resolves; rows that contradicted
      their notes or their milestone corrected (list in
      docs/audits/2026-11-14-m32-parity-audit.md).
- [x] Close what was cheap: Findings → Issue definitions (TARGET-006,
      `IssueDefinitionsDialog.tsx`; `PassiveCheck` now carries
      `description` and `remediation`), a test for Find references.
- [x] Regenerate the summary by counting rows
      (`node scripts/matrix-summary.mjs`): 87 🟢 / 41 🟡 / 4 🔵 / 2 ➖.
- [x] ROADMAP row 32, CHANGELOG. **Parity is not declared.**

## Done: quality pass (2026-11-13/14, on main, three commits)

- Bugs fixed (each with a test unless said): API connections served
  after the API stopped; API and OAST accept loops spinning on errors (no
  test: the error cannot be provoked in a test); overlapping loads in the
  Organizer, OAST and plugin log stores; two ChosAI streams with one name.
- Intermittent tests: `browser_session` read the arguments file before it
  was written; the live crawl test waited for one hit and asserted on
  another; `pools.rs` had an absolute upper time limit. The timing
  assertions left are lower bounds or generous.
- Duplication removed: `tls_security` (five copies), `lock` (four),
  `read_shared_file` / `write_shared_file` (three), `site_marks`.
- Performance (docs/PERFORMANCE.md): startup script 866 → 339 kB (the
  engagement dialogs are lazy; `scripts/check-bundle.mjs`), index on
  `exchanges(response_len)` (schema v27). An index on `duration_ms` was
  measured and not kept (inserts 40 % slower).
- Not done: React re-renders were read, not measured in the running app.

## Previous milestone: M31b — Deferral sweep (done, on main)

The items earlier milestones parked on M31 that are not hardening (list in
ROADMAP "Milestone 31"). Matrix rows touched: AI-004, PLATFORM-003,
EXT-002, INTRUDER-006 / PLATFORM-001 (pool choice), PROXY-001 / PROXY-014
(per-listener certificate), PROXY-017 (NTLM), SESSION-007 (OS keychain),
OAST-001 (SMTP), TARGET-007 (crawl graph), ORGANIZER-002 and REPEATER-002
(drag and drop, nesting, colours), TARGET-004 (site map conditions),
EDITOR-001 (GraphQL in Intercept), SETTINGS-003, SCRIPT-003 (WebSocket
scripts).

**Scope decision (keep it):** like every milestone since M25, nothing here
adds attack payloads, active checks or traffic the tester did not ask for
(M13 remains pending). New listeners (SMTP) are off by default and
loopback-only unless chosen. Credentials stay in the keyring (or the OS
keychain when chosen), never in config files or logs. Anything that talks
to an outside service needs an explicit opt-in.

Plan (one commit per step, in this order; tick as they land):

- [x] **A. ChosAI and ChosAgent follow-ups** — done. Streaming:
      `chosproxy_request::send_streaming` / `Connection::send_streaming`
      (`BodySink`; `BodyReader::next_is_data` skips chunk framing),
      `crates/ai/src/stream.rs` (`stream_body`, `StreamParser`: Ollama
      NDJSON, OpenAI SSE, falls back to a whole reply), engine
      `ai_run_stream(request, approved, stream, on_delta)` and
      `ai_cancel(stream)` (`Inner.ai_streams`, at most 8; a cancelled call
      is `AppError::Cancelled` and logs nothing); Tauri `ai_run_stream`
      (events `ai://delta` `{stream, text}`), `ai_cancel`; AiDialog shows
      the answer while it is written (`deltaBuffer`, 60 ms) with Stop; lab
      model streams when asked (`"stream": true`; model `slow` waits
      between pieces). Agent: `agent_continue(task, message)` starts a new
      task with the same goal and items, seeded with `earlier_summary` of
      the old task's activity log (redacted, 12k characters, activity row
      `resume`); refused while the old task runs; AgentView "Continue in a
      new task" and an "Interrupted" note (`interrupted()` in
      `state/agent.ts`). Record login: `login_recording_start / stop`,
      `login_recording` (a history mark in `Inner.login_recording`, cleared
      when a project opens; `pick_login_requests`: proxy requests only, no
      static files, non-GET first, at most 20; storage
      `proxy_exchanges_after`); AiDialog "Record login" for the login-flow
      task, palette "Record a login for ChosAI". Tests: `stream.rs` unit
      tests, `tests/ai.rs` (streamed in both formats, cancel, recording),
      `tests/agent.rs` (continue), `state/ai.test.ts`, `state/agent.test.ts`.
      Not seen in the running app; not tried against a real Ollama.
- [x] **B. Automation API** — done: `crates/api/src/routes.rs` (`ROUTES`,
      the one route table; `openapi(port)` builds the OpenAPI 3.1 document;
      tests check the table against the router's `(&Method::…)` arms and,
      over HTTP, that every listed route is answered); `GET
      /v1/openapi.json`; `GET /v1/events` (`events()` in `lib.rs`:
      Server-Sent Events `event: <type>` / `data: <EngineEvent JSON>`,
      token + `Origin`/`Host` checks as every route, at most
      `MAX_EVENT_STREAMS` = 8, a 256-event queue per client and the client
      is dropped when it is full, `event: lagged` after missed events, a
      `: ping` comment every 15 s, streams end when the server stops,
      `Connection: close`); responses are now `BoxBody`; CLI `events` (one
      JSON object per line) and `openapi`. Tests in `crates/api/tests/api.rs`
      and the CLI test. Not tested: the drop of a client that stops reading
      (needs megabytes of events to fill the socket buffers).
- [x] **C. Plugins and pools** — done. Hooks: `RequestHook` gained
      `wants_response` / `on_response` (defaults: no); the HTTP/1 relay
      holds a response a hook wants (same hold limit as Intercept), after
      Match & Replace and before Intercept (`Replace`/`Respond` deliver
      other bytes, `Block` = 502 with the reason, flag DROPPED); the HTTP/2
      relay (`h2_relay.rs`) now runs request hooks (after Match & Replace,
      before Intercept; the request in its `GET / HTTP/2` text form) and
      response hooks. Plugin protocol: `registerHook {hosts, responses}`,
      `hook {call, request, response?}` (a `response` makes it a response
      hook; answers `continue` / `replace` with `response` / `block`);
      host `plugin_hooks.rs`: `ask_hook` (shared 2 s wait, fails open),
      `run_response_hooks`, `unchunked` (plugins read a chunked body in one
      piece; untouched responses go on byte for byte), `replacement` keeps
      HTTP/2 text as it is. Same permission (`proxy.intercept`), scope
      rule and notes. Tests: `crates/proxy/tests/http2.rs`
      (`hooks_see_http2_requests_and_responses`), the plugin hook test in
      `crates/app/tests/plugins.rs`, unit tests. Pools:
      `task_set_pool(task, pool)` chooses the pool a *running* task sends
      through from its next request (kept in `RunningTask.pool`, gone when
      the task ends; `TaskDetail.pool`); `pool_enter` looks at the task of
      the current future (`CURRENT_TASK` task-local, re-scoped with
      `in_task` where crawl, fuzz and discovery spawn their sends); Tauri
      `task_set_pool`; a "Pool" select in the task panel
      (`poolChoiceFor`). Test `a_running_task_can_be_given_its_own_pool`.
      Not done: choosing the pool in the launch dialogs (the first requests
      of a task use the tool's assignment until the tester picks).
- [x] **D. Network and credentials** — done. Certificate per listener:
      `ListenerConfig.certificate` (`ListenerCertificate`: `shared` |
      `ownCa` | `file {cert, key}`); `Interceptor::with_certificate` (one
      chosen PEM certificate for every host; the key must match),
      `authority_shared`; proxy `set_listener_tls(id, interceptor)`,
      `Shared::interceptor(listener)` and the download page give the
      listener's CA; `CaManager::listener_interceptor` (own CA in
      `ca/listeners/<id>/`, made once and kept); engine
      `start_proxy_listener` (a certificate that cannot be loaded keeps the
      listener from starting); UI select in Proxy → Listeners. The lab
      browser trusts the shared CA only. NTLM: `crates/request/src/ntlm.rs`
      (NTLMv2 only; `md4`, `md-5`, `hmac` crates; checked against the
      [MS-NLMP] §4.2.4 example), `AuthScheme::Ntlm`, `ntlm_send` in
      `send.rs` (negotiate → challenge → answer on one connection),
      `with_authorization`; lab `/auth/ntlm` (user `lab` / `secret`,
      written independently, the answer must come on the connection that
      got the challenge; `ConnId` request extension); Settings → Network
      scheme "NTLMv2". Not for requests that reuse a Repeater connection or
      HTTP/2; Kerberos stays out. Keyring: `KeyringProtection` (`file` |
      `osKeychain` | `password`) stored in `credentials.enc` itself
      (version 2; version 1 files = key file); `OsKeychain` trait
      (`SystemKeychain`: macOS Keychain through `security-framework`, item
      "ChosProxy keyring", one account per data directory; not available on
      other systems yet; `MemoryKeychain` for tests,
      `Engine::use_memory_keychain`); master password = PBKDF2-HMAC-SHA256
      600k (ring), the derived key kept in memory for the session
      (`Inner.keyring_key`); engine `keyring_status / unlock / lock /
      set_protection`; Tauri commands; Settings → Credentials "Keyring
      protection" card (`KeyringProtectionCard.tsx`). Tests:
      `crates/proxy/tests/http2.rs` (own CA), `leaf.rs`,
      `tests/listener_cert.rs`, `ntlm.rs`, `tests/platform_auth.rs`,
      `keyring.rs`, `tests/keyring_protection.rs`, UI tests. **Not tested:**
      the real macOS Keychain (tests use the memory one, so they never touch
      the tester's keychain); NTLM against a real Windows server. **Needs a
      decision:** Secret Service on Linux (a D-Bus dependency in the Kali
      build).
- [x] **E. ChosOAST SMTP** — done: `crates/oast/src/server.rs`
      `ServerConfig.smtp`, `Protocol::Smtp`, `serve_smtp` / `handle_smtp`
      (a sink: HELO/EHLO, MAIL FROM, RCPT TO (20 at most), DATA, RSET,
      NOOP, QUIT; AUTH, STARTTLS and the rest answer 502; 64 commands, 10 s
      per read, the mail kept up to `MAX_RAW` and read up to ten times
      that; one interaction per mail with the envelope and the body;
      nothing is delivered or relayed), `smtp_addr`; `LocalOast.smtp_enabled`
      (off) / `smtp_port` (2525); `chosoast --smtp <addr:port>` (off unless
      given); engine `OastStatus.smtp_listener`, finding "External service
      interaction (SMTP)"; OAST view switch and port. Tests: the server
      unit test and `crates/app/tests/oast.rs`. Not tried with a real mail
      server delivering to it (port 25 and an MX record are the tester's to
      set up).
- [x] **F. UI pass** — done, in four commits. Crawl graph:
      `engine/crawl_graph_ops.rs` (`crawl_graph(task)`, read from the
      crawler's notes with `parse_crawl_note`; storage `crawl_exchanges`),
      `lib/crawlGraph.ts` (`layoutCrawlGraph`: columns by depth),
      `features/tasks/CrawlGraphView.tsx` in the task panel of crawls and
      scans. Organizer: collections nest with `/` (`normalize_collection`,
      4 levels, no schema change), `lib/collections.ts`, a collection tree
      with drag and drop (`moveTo` in the store). Repeater: tabs dragged
      onto tabs (place and group) and onto group chips, a colour per tab
      (migration v26 `repeater_tabs.color`, `repeater_set_color`, tab menu
      "Highlight"). Site map conditions: `site_map_conditions(query,
      source)` (`ScriptedSiteMap`, `site_entries` from the rows the filter
      keeps; same bound as the history conditions), the condition builder
      reused through a `binding` prop. Intercept: "Format body" /
      "Compact" and a GraphQL panel (query and variables pretty and
      editable, written back with `withGraphql` + `replaceBody`).
      Configuration library: `engine/toolconfig_ops.rs` (`toolconfigs.toml`,
      `tool_configs / save / delete / export / import`, settings only: no
      target or request), `lib/toolConfigs.ts`, `ConfigLibraryBar` in the
      crawl dialog, the discovery dialog and the Fuzzer. Tests:
      `tests/crawl.rs`, `tests/scripts.rs`, `tests/tool_configs.rs`,
      storage tests, `lib/*.test.ts`, `state/repeater.test.ts`. Nothing of
      this was seen in the running app (drag and drop above all needs a
      look).
- [x] **G. Scripts** — done: Match & Replace target
      `MatchTarget::WsScript` ("WebSocket script": `match` holds the id of a
      rewrite script); rewrite scripts may define `fn websocket(text,
      to_server)` (`Script::websocket`, entry-point arity 2; returns the
      new text or `()`); `MatchReplace::apply_websocket` runs text rules and
      script rules in rule order on text messages, a failing script leaves
      the message as it is (logged); the UI target and the rewrite
      template. Tests: `match_replace.rs`
      (`websocket_script_rules`), `crates/script` unit test. Binary
      WebSocket messages are not given to scripts. Seen while checking:
      `a_live_crawl_lists_unrequested_urls_and_resumes_after_reopening`
      (`tests/live.rs`) failed once in a full run and passed 6 times alone
      — one more timing-sensitive test for the quality pass.
- [x] **H. Docs and checks** — done: ADR-0042, ROADMAP ("Milestone 31b
      (done)" with its Deferred list), CHANGELOG, README, THREAT_MODEL
      T77–T83, docs/API.md, AI.md, SCRIPTS.md, OAST.md, NETWORK.md,
      PLUGIN_SDK.md; matrix rows updated and the summary regenerated by
      the new `scripts/matrix-summary.mjs` (83 🟢 / 43 🟡 / 6 🔵 / 2 ➖;
      `--check` verifies the table against the rows).

**Not planned here (each needs a decision first):** runtime
instrumentation INSTR-001 (design open since ADR-0035); ACME certificates
for a self-hosted OAST server (talks to an outside CA); raw and streaming
HTTP/2 frames in Repeater (large; its own milestone if wanted);
GraphQL-specific checks (only passive checks on observed traffic would fit
the rules); other payload generators; replaying recorded browser
interactions as logins; model and OAST providers through plugins (they
would hand traffic to plugin processes); script data-flow analysis (no
clear use yet).

## Previous milestone: M31 — Hardening / Recovery / Encryption (partly done, on main)

Matrix rows: PROJECT-004 (crash recovery), PROJECT-005 (encrypted
projects), ORGANIZER-003 (encrypted sharing of collections), PLATFORM-006
(performance profiler), PLATFORM-002 (persistence of the event log, via the
project audit log), plus spec §117 (temporary files), §120 (Security
Center) and §122 (audit log coverage). Spec: PROJECT_SPEC_ADDENDUM_PARITY.md
§117–§122.

**Scope decision (keep it):** hardening only, no new ways to send traffic.
Encryption uses audited primitives only — SQLCipher for project databases
(through rusqlite), `ring` (PBKDF2-HMAC-SHA256 + AES-256-GCM) for exported
files — never home-made cryptography. Passwords are never stored or
logged; a lost password means a lost project (said plainly in the UI).
Recovery never restarts a task that sends traffic by itself, and cleanup
never deletes anything outside ChosProxy's temp directory or anything that
is not a ChosProxy temp file. The profiler and Security Center are local
views: nothing is sent anywhere.

**Split:** the ROADMAP also parks many unrelated deferrals on M31 (AI
streaming, AI-004 login recording, agent tasks surviving a reopen, model
and OAST providers through plugins, SMTP/ACME OAST, raw/streaming HTTP/2,
HTTP/2 plugin hooks, plugin OS sandbox, OS keychain, NTLM, INSTR-001, crawl
graph TARGET-007, configuration library for other tools SETTINGS-003,
Organizer nested collections and drag and drop, per-listener certificates,
GraphQL checks and Intercept pretty editing, OpenAPI description and event
streaming of the automation API, script data-flow analysis, WebSocket
payload processing, global pool choice). They move to **M31b** (deferral
sweep, planned after M31) in M31's docs step, so M31 stays a coherent
hardening milestone.

Plan (tick and commit as steps land):

- [x] **A. Crash recovery (PROJECT-004)** — done. Session markers
      (`sessions/<session_id>.json`, `0600`) with PID + process start time
      liveness detection (`sysinfo`), `SessionHandle` drop tracking,
      `recovery_status`, `recovery_restore`, `open_project_read_only`,
      `recovery_open_read_only`, `recovery_discard`, `ensure_writable`
      across engine operations, `tasks_interrupted_crash`, temp cleanup
      protection for recoverable temp files, Tauri commands, startup
      `RecoveryDialog` and read-only banners/badges; tests in
      `crates/app/tests/recovery.rs` and `RecoveryDialog.test.tsx`. Already implemented:
      storage read-only open (`OpenOptions.read_only`,
      `StorageError::ReadOnly`, `ProjectDb::is_read_only`,
      `ProjectDb::quick_check`, copies from a read-only project; test
      `read_only_projects_read_but_never_write`). Original plan: a session marker
      `data_dir/session.json` (pid, start time, project path and kind,
      running tasks), written when a project opens and when tasks start or
      end, removed on a clean close and at exit. On start, a marker whose
      process is gone means "Previous session ended unexpectedly." with:
      **Restore** (reopen the project after `PRAGMA quick_check`; tasks
      left `running` are recorded as interrupted, live passive tasks resume
      as today, nothing that sends restarts by itself), **Open read-only**
      and **Discard recovery** (forget the marker; an orphaned
      `temp-*.chosproxy` from that session is deleted only after asking).
      Read-only projects: `OpenOptions.read_only` (SQLite read-only, no
      writer thread, writes refused with a clear error); the engine refuses
      listeners, sends and tasks while read-only and the UI shows a banner.
      Engine `recovery_status`, `recovery_restore / open_read_only /
      discard`; tests in `crates/app/tests/recovery.rs` (simulated stale
      marker, interrupted tasks, read-only refusals). Tauri commands and a
      startup dialog.
- [x] **B. Encrypted projects (PROJECT-005)** — done. Spike passed:
      rusqlite `bundled-sqlcipher-vendored-openssl` (SQLCipher 4.20
      community; storage build 33 s on macOS; Kali release build of
      `chosproxy-cli` 9 min 25 s under emulation, no dynamic OpenSSL or
      SQLite); plain projects unchanged (all tests); WAL, `VACUUM INTO`
      copies and backups stay encrypted; wrong / missing key fails; rekey
      and `sqlcipher_export` both ways work. The Kali Dockerfile now
      installs rustfmt and clippy (rustup otherwise tried to add them in
      the container and failed; the full `scripts/build-kali.sh` was still
      not run). Code: `crates/storage/src/key.rs` (`ProjectKey::password`,
      `::random`, `is_encrypted_file`), `OpenOptions.key`, `rekey`,
      `copy_with(dest, in_scope, CopyKey::Same|Plain|Key)`, `inspect_copy`,
      `inspect_project_file_with`, `StorageError::{Encrypted, WrongKey}`
      (IPC kinds `encrypted`, `wrongPassword`); engine
      `create_project_with`, `open_project_with` (a wrong password leaves
      the open project open), `new_temporary_project_with(encrypt)`,
      `save_project_as_with` / `export_project_with(ProjectEncryption)`,
      `change_project_password`, `inspect_project_with`,
      `restore_backup_with`; `ProjectInfo.encrypted`; audit
      `project.encrypted` / `project.decrypted` /
      `project.password.changed`; API `password` field on project
      open/create; CLI `CHOSPROXY_PROJECT_PASSWORD` or `--password-file
      FILE|-`; UI `PasswordDialog` + `state/password.ts` (unlock / choose /
      change, `withPassword`), Welcome "Encrypted temporary project",
      Project view badge and "Change password…". Tests
      `crates/storage/tests/encryption.rs`, `crates/app/tests/encryption.rs`,
      `state/password.test.ts`. Not seen in the running app. Original plan: start with a spike: switch
      rusqlite to SQLCipher (`bundled-sqlcipher-vendored-openssl`), check
      that plain projects still open unchanged, that WAL and `VACUUM INTO`
      copies and backups stay encrypted, and that the macOS build and
      `scripts/build-kali.sh` still work; record build time. If the spike
      fails, stop and ask the user (no fallback to own crypto). Then: new
      project / Save As "Encrypt with a password" (SQLCipher defaults:
      PBKDF2-HMAC-SHA512, 256k iterations), opening asks for the password,
      change password (`PRAGMA rekey`), remove encryption (export a plain
      copy, explicit confirmation); `inspect_project_file` reports
      "encrypted" without a password; backups and exported copies keep the
      encryption; temporary projects can be encrypted with a random key
      held in memory. Password never stored, logged, sent to the UI after
      entry, or put in the API. Tests in storage (encrypted round trip,
      wrong password, rekey, copies) and the engine; CLI/API open with a
      password from stdin or an env var only.
- [x] **C. Encrypted exports (ORGANIZER-003)** — done: atomic `0600`
      writes (`write_private_atomic`), nested collection prefix matching on
      export, `is_sealed_file` inspection, Tauri commands
      (`organizer_export`, `organizer_import`, `is_sealed_file`, and
      password support on `export_settings`, `preview_settings_import`,
      `apply_settings_import`), `withSealedFilePassword` in
      `state/password.ts`, UI in `OrganizerView.tsx` and
      `SettingsTransferCard.tsx`, and tests in
      `crates/app/tests/encrypted_share.rs`, `OrganizerView.test.tsx`,
      `SettingsTransferCard.test.tsx` and `password.test.ts`. Existing implementation: `crates/storage/src/sealed.rs`
      (`seal`, `open`, `is_sealed`; PBKDF2-HMAC-SHA256 600k + AES-256-GCM,
      authenticated versioned header; unit tests incl. a published PBKDF2
      vector), `organizer_insert_all` / `NewOrganizerItem` (all or none),
      `engine/share_ops.rs` (`organizer_export(path, collection, password)`,
      `organizer_import(path, password)`, `OrganizerShare`), encrypted
      settings exports (`export_settings_with`, `preview_settings_import_with`,
      `apply_settings_import_with`, `projects::{seal_if, unseal}`),
      `[profile.dev.package.ring] opt-level = 3`. Original plan: Organizer collections
      (and settings exports) can be exported as an encrypted file:
      PBKDF2-HMAC-SHA256 (600k iterations, random salt) + AES-256-GCM with
      `ring`, a versioned header, authenticated; import asks for the
      password and fails cleanly on a wrong one or a modified file. Pure
      code in a small `crates/core` or `crates/storage` module with test
      vectors; UI in Organizer and Settings → Import/Export.
- [x] **D. Temporary files (§117)** — done: `engine/temp_ops.rs`
      (`temp_status`, `temp_cleanup` = this run's unused leftovers,
      `temp_remove(names)` = only old (>24 h) leftovers of other runs,
      `temp_cleanup_on_exit` in `shutdown`; kinds by name only:
      `temp-*.chosproxy[-wal|-shm]`, `browser-*` dirs, `api-report-*.tmp`;
      the open project and the lab browser profile are in use; links and
      other files are never touched; audit `temp.cleaned` / `temp.removed`);
      Tauri `temp_status` / `temp_cleanup` / `temp_remove`; Settings →
      Storage (`StorageSection.tsx`); tests `crates/app/tests/temp_files.rs`
      (decoys that must survive), `StorageSection.test.ts`. No cache exists,
      so there is no cache-size setting. Side fix: `h2.rs` test timing
      assertion (ring at opt-level 3 makes a loopback handshake < 1 ms).
      Original plan: Settings → Storage: the temp
      directory, its size by kind (temporary projects, lab browser
      profiles, DOM/browser-crawl profiles, report/export staging if any),
      cleanup on exit and on start (only ChosProxy's own patterns inside
      its temp dir, never a project the recovery marker points at, never a
      persistent project), "Clean now" with a list of what goes. Engine
      `temp_status`, `temp_cleanup`; tests with decoy files that must
      survive.
- [x] **E. Performance profiler (PLATFORM-006)** — done:
      `engine/perf_ops.rs` (`performance()` → `PerformanceSnapshot`:
      process memory and CPU via `sysinfo` 0.38 (`system` feature only,
      this process only), database file + WAL size, `stored` counter fed by
      the insert hook (reset per project), dropped captures, `ProxyStats`,
      pool in flight / waiting, running tasks); Tauri `performance`; view
      Tools → Performance (`features/performance/PerformanceView.tsx`,
      reads every 2 s while open, 90 readings in memory, sparklines;
      `lib/perf.ts` `storedRate`, `pushReading`, `sparkline`); palette
      "Open Performance"; tests `crates/app/tests/performance.rs`,
      `lib/perf.test.ts`. Not seen in the running app. Also: 26 iCloud
      duplicate files (`* 2.rs/.ts/.tsx/.md/.sql`, untracked, identical
      to tracked files or older) were moved out of the repo into the
      session scratchpad. Original plan: engine `performance()`:
      process memory and CPU (via `sysinfo`, process only), database size
      (file + WAL), history count and capture rate (inserts per second over
      the last minute), proxy connections open, requests waiting and in
      flight in resource pools, running tasks, event channel drops; Tools
      → Performance with a short in-memory history (sparklines) refreshed
      every 2 s while open. Local only. Tests for the counters.
- [ ] **F. Security Center (§120) and audit coverage (§122)** — pending.
      Planned engine `security_overview()` returning checks with ok / warn / info and a
      link to the setting: CA status and private-key location, listeners
      bound beyond loopback and whether they need auth, automation API,
      plugins and their granted permissions, signed-only plugins, ChosAI
      mode and provider (external = warn), ChosAgent permissions set to
      Allow, upstream proxies and OAST mode (outside connections), TLS
      verification exceptions, project encryption, temp cleanup. View
      Settings → Security & Privacy (or its own view). Audit log: add the
      missing §122 events — plugin permission granted/revoked, scans,
      crawls and other traffic tasks started/stopped, ChosAgent approvals
      and rejections, recovery actions, encryption on/off/rekey — and an
      Audit log viewer with filters in Project (PLATFORM-002: it persists
      in the project). Tests per check and per event.
- [x] **G. Docs and checks** — done for what M31 delivered:
      docs/PROJECT_FILES.md (encrypted projects, CLI/API passwords,
      temporary files, Performance view), docs/API.md (`password`),
      ADR-0041, matrix (73/52/7/2: PROJECT-005 and PLATFORM-006 🟢,
      ORGANIZER-003 🟡, PROJECT-004 still 🔵 with a note), ROADMAP (M31 🚧
      with pending-work and "Moved to M31b" lists, new row 31b), changelog,
      README, threat model T73–T76. Checks: 748 Rust tests, 189 vitest
      after step E (no code changed in G). Original plan: docs (a `docs/SECURITY_CENTER.md` or a
      section in SECURITY.md; projects/encryption and recovery in the user
      docs), ADR-0041, matrix, roadmap (M31 ✅ and the M31b deferral list),
      changelog, README, threat model (crash leftovers, lost password, key
      in memory, encrypted export tampering, temp cleanup deleting the
      wrong file).

## Previous milestone: M30 — ChosAgent (done, on main)

Matrix rows: AI-005 (agentic testing with approval controls), AI-006 (AI
activity log), AI-004 (apply the suggested session rule), plus the M29
deferrals (conversations; streaming). Spec §107–§108
(PROJECT_SPEC_ADDENDUM_PARITY.md).

**Scope decision (keep it):** ChosAgent can only call ChosProxy's own
deterministic tools from a fixed catalog; there is no shell, file or OS
access, and a tool name outside the catalog is refused. Every call goes
through one dispatcher in the engine that enforces DENY / ASK / ALLOW
(not the prompt). Tools that send traffic are capped at ASK (the tester
sees and approves the exact request; ALLOW cannot be set), need a
*defined* scope that contains the target, and have a per-task send budget.
The agent writes no attack payloads (system prompt) and there are no
payload lists, active checks or scan launches among its tools — this is
not the active audit (M13 remains pending). With an external model, every
model call needs the tester's approval of the exact new context (the M29
preview rule, per step). Everything is recorded in the AI activity log;
the agent's requests are in the history with source `agent`.

Plan (tick and commit as steps land):

- [x] **A. Agent protocol in `crates/ai`** — done: `crates/ai/src/agent.rs`
      (`TOOLS`, `tool`, `effective`, `validate_config`, `tool_infos`,
      `offered`, `AgentMessage`/`ToolCall`/`AgentReply`, `system_prompt`,
      `goal_message`, `agent_body`, `parse_agent_reply`, `compact`,
      `agent_digest`); `AgentConfig`/`AgentPermission` in
      `crates/core/src/config/agent.rs`. Original plan (`agent.rs`, pure): the tool
      catalog (name, description, JSON parameters, kind read / project /
      send, default and highest permission), `AgentPermission`
      deny/ask/allow and `effective()` (caps), the agent system prompt,
      conversation messages with assistant tool calls and tool results,
      chat bodies with `tools` (Ollama and OpenAI-compatible) and reply
      parsing (native tool calls, or a fenced JSON block as fallback);
      `AgentConfig` in `agent.toml` (permissions, max steps, max sends).
      Tools: `search_history`, `get_exchange`, `list_findings`,
      `get_finding`, `get_scope`, `analyze_exchange` (passive checks on a
      stored exchange), `decode`, `update_plan` (subtasks), `add_note`,
      `draft_finding`, `propose_session_rule`, `send_request`.
- [x] **B. Storage and engine** — done: migration v25 + `crates/storage/src/ai_activity.rs`
      (`ai_activity_add`, `ai_activity(task, after, limit)`; rows go with
      the task); `Source::Agent`; `EngineEvent::AgentChanged`;
      `engine/agent_ops.rs` (`agent_config`/`save_agent_config`,
      `agent_tools`, `agent_start`, `agent_state`, `agent_approvals`,
      `agent_decide`, `agent_message`, `ai_activity`; `agent_dispatch` →
      `agent_prepare` → approval → `agent_run`; `agent_send`); ChosAI
      one-shot calls log `ask` rows; `ai_post` shared with ChosAI; lab
      `/agent/api/chat`, `/agent/v1/chat/completions` (scripted; model
      `text-only` refuses native tools); test `crates/app/tests/agent.rs`;
      Tauri commands `agent_*`, `ai_activity`. Original plan: migration v25 `ai_activity` (task,
      time, action, tool, target, permission, decision, result, detail),
      `Source::Agent`; `engine/agent_ops.rs`: task kind `agent`
      (`agent_start(goal, attachments)`, model ↔ tool loop with step
      limit, pause / resume / stop through the task controls, queued
      tester messages, pending approvals `agent_approvals` /
      `agent_decide`, outgoing-context approvals for external models),
      the dispatcher, ChosAI one-shot calls logged too; lab scripted
      stand-in agent model; tests `crates/app/tests/agent.rs`; Tauri
      commands.
- [x] **C. ChosAI conversations and session rules** — done:
      `AiRequest.conversation` (alternating answers / follow-ups, the
      tester's redacted, inside the digest); login-flow prompt ends with a
      ```` ```chosproxy-session-rule ```` JSON block; `sessionRuleFrom` and
      `useSessionDraft` in `state/ai.ts`; AiDialog thread + follow-up box +
      "Review as session rule…"; SessionRulesCard takes the draft (unsaved);
      session rule tool `agent`. Original plan: follow-up
      questions in the ChosAI dialog (earlier turns sent again, inside the
      digest); the login-flow answer ends with a JSON session rule that
      the dialog offers to open in Project → Session rules for review.
- [x] **D. UI** — done: view `agent` (`features/agent/AgentView.tsx`:
      new task, task list, plan, approval cards with the exact request /
      context and a note, transcript, message box, pause / resume / stop;
      tab AI activity log), `state/agent.ts` (`useAgentStore`,
      `transcript`), nav entry with a waiting-approvals count, palette
      "Open ChosAgent", request menu "Give to ChosAgent…",
      `agentChanged` events; Settings → ChosAgent (`AgentSection.tsx`,
      `choicesFor` caps the choices); tests `state/agent.test.ts`. Original plan: ChosAgent view (start with a goal and attached ids,
      transcript, plan, pending approvals with the exact arguments,
      approve / deny, queued messages, pause / resume / stop); AI activity
      log (filter by task); Settings → ChosAgent (permission per tool,
      capped choices, limits).
- [x] **E. Docs and checks** — done: docs/AI.md (ChosAgent, conversations,
      session rules, activity log), ADR-0040, matrix (71/51/10/2), roadmap
      (streaming deferred → M31), changelog, README, threat model T70–T72.
      All checks green (727 Rust tests, 182 vitest). Not tried against a
      real Ollama/LM Studio, and the UI was not seen in a running app (the
      lab's scripted model covers the loop end to end). Original plan: docs/AI.md (ChosAgent), ADR-0040, matrix,
      roadmap (streaming answers deferred → M31 unless it fits: the
      Request Engine reads whole responses), changelog, README, threat
      model.

## Previous milestone: M29 — ChosAI (done, on main)

On `main`. Matrix rows: AI-001 (explain this), AI-002 (AI prompts in
Repeater), AI-003 (finding analysis), AI-004 (login helper). Spec §106,
§109–§111 (PROJECT_SPEC_ADDENDUM_PARITY.md), §60 (PROJECT_SPEC.md).

**Scope decision (keep it):** ChosAI only produces text — explanations,
summaries, suggested manual checks, report wording. It never sends traffic
or acts (that is ChosAgent, M30, with DENY/ASK/ALLOW). Off by default; a
local model is first-class (Ollama, OpenAI-compatible servers such as LM
Studio or llama.cpp on loopback); an external provider is opt-in. Never
the whole project: only the attached items, cut to a size limit, with
secrets redacted (Authorization, cookies, tokens, passwords, API keys,
custom regexes). Before anything goes to an external provider the tester
sees the exact context, and the engine refuses an external call whose
context was not previewed (hash check). No telemetry. It does not write
exploit payloads.

Plan (tick and commit as steps land):

- [x] **A. `crates/ai`** — done: `redact.rs` (`redact`, `RedactRules`;
      auth headers, cookies, Set-Cookie, JWTs, secret parameters and JSON
      fields, known key formats, private keys, custom regexes), `lib.rs`
      (`exchange_item`, `assemble`, `AiTask`, `SYSTEM_PROMPT`, `messages`,
      `digest`, `chat_path`, `chat_body`, `parse_reply`); `AiConfig` in
      `crates/core/src/config/ai.rs`. Original plan (pure): redaction rules and report; context
      builders (request/response, finding, JavaScript selection, several
      exchanges for the login helper) with size limits; tasks and their
      prompts (explain request, response, finding, JavaScript, login flow,
      free question, draft finding text); Ollama `/api/chat` and
      OpenAI-compatible `/v1/chat/completions` bodies and replies.
- [x] **B. Engine and settings** — done: `engine/ai_ops.rs` (`AiAttachment`
      exchange/finding/text, `AiRequest`, `AiPreview` with `digest` and
      `external`, `AiAnswer`; `ai_config`, `save_ai_config`, `ai_preview`,
      `ai_run(request, approved_digest)`, `ai_test`; API key from the
      keyring via `credential_secret`); lab stand-in model
      `POST /ai/api/chat` and `/ai/v1/chat/completions` (echoes the last
      message); test `crates/app/tests/ai.rs`; Tauri `ai_*` commands.
      Original plan: `ai.toml` (`AiConfig`: mode
      disabled/local/external, provider, base URL — loopback only in local
      mode — model, keyring credential for the key, timeout, context
      limit, custom redaction patterns); `engine/ai_ops.rs`
      (`ai_preview`, `ai_run` with the preview hash for external,
      `ai_test`), sent through the Request Engine (network settings),
      not stored in the history, audited; lab fake model routes; tests.
- [x] **C. UI** — done: `features/settings/AiSection.tsx` (mode, local
      presets Ollama/LM Studio/llama.cpp, API, URL, model, key from the
      keyring, timeout, limit, patterns, test); `features/ai/AiDialog.tsx`
      mounted in the shell (`state/ai.ts`: `open`, `tasksFor`, `parseIds`;
      preview, external sends carry the preview digest, answer as
      Markdown); entry points: request menu "Explain with ChosAI…",
      finding editor "ChosAI…", Repeater "ChosAI", message viewer selection
      "ChosAI"; login flow takes extra request ids. Original plan: Settings → ChosAI; a ChosAI dialog (task, question,
      context preview with redaction counts, answer as Markdown) from the
      request menu ("Explain with ChosAI…"), Repeater, the finding editor
      and the DOM/JS selection.
- [x] **D. Docs and checks** — done: docs/AI.md, ADR-0039, matrix
      (69/51/12/2), roadmap, changelog, README, threat model T67–T69. All
      checks green (701 Rust tests, 179 vitest). Not tried against a real
      Ollama/LM Studio here (the lab stand-in model covers both formats).

## Previous milestone: M28 — REST API / Headless (done, on main)

On `main`. Matrix rows: PLATFORM-003 (REST API: off by default, auth,
loopback) and PLATFORM-004 (headless / CLI).

**Scope decision (keep it):** the API exposes what the UI already does,
with the same limits (scope checks, safe defaults, no attack payloads).
It is off by default, binds to loopback only, needs a token on every call,
refuses browser requests (an `Origin` header) and foreign `Host` headers
(DNS rebinding), and audits the calls that change something. The OAST
items deferred into M28 (SMTP/ACME, OAST providers via plugins) move to
M31: they open new listeners or talk to outside services and are not part
of the API/CLI.

Plan (tick and commit as steps land):

- [x] **A. Automation API** — done: `ApiConfig` (`crates/core/src/config/api.rs`);
      engine `engine/automation_ops.rs` (`api_config`, `save_api_config`,
      `paths`, `audit_automation`, `automation_send`); crate `crates/api`
      (`ApiServer::start(engine, port, token)`, `token::{load_or_create,
      read, regenerate}`, routes under `/v1/` — see the match in
      `crates/api/src/lib.rs`); tests `crates/api/tests/api.rs`. Original plan: `api.toml` (`ApiConfig`:
      enabled = false, port 8790) in core; a token in
      `data_dir/api.token` (random 32 bytes, private file, regenerable);
      new crate `crates/api` (hyper HTTP/1.1 on 127.0.0.1, JSON, `/v1/…`):
      status, project open/new, listeners start/stop, history page and
      exchange, findings, scope get/put and add URL, crawl, scan and scan
      configs, tasks list/detail/pause/resume/cancel, Repeater-style send,
      API definition import, report export. Token compared in constant
      time; body limit; `Origin` refused; `Host` must be loopback.
- [x] **B. `chosproxy-cli`** — done: `crates/cli/src/main.rs` (`serve`
      prints "API listening on http://…" first; client commands over a
      plain HTTP/1.1 call to 127.0.0.1); end-to-end test
      `crates/cli/tests/cli.rs` runs the binary in a scratch
      `CHOSPROXY_HOME` (with `listeners = []`). Original plan: new crate `crates/cli`:
      `serve` (headless: engine + project + API + listeners, no UI) and
      client commands over the API (`status`, `history`, `show`,
      `findings`, `scope`, `crawl`, `scan`, `tasks`, `wait`, `cancel`,
      `report`, `import-api`); token from `--token`,
      `CHOSPROXY_API_TOKEN` or the token file.
- [x] **C. Desktop** — done: `apps/desktop/src-tauri/src/automation.rs`
      (`Automation::restart/status`, started in `setup` when enabled),
      `ApiStatus` in `automation_ops.rs`, commands `api_status`,
      `save_api_settings`, `regenerate_api_token`; Settings → Automation API
      (`AutomationSection.tsx`). Original plan: the desktop starts the API when enabled; Settings →
      Automation API (switch, port, token copy/regenerate, status).
- [x] **D. Docs and checks** — done: docs/API.md, ADR-0038, matrix
      (66/50/16/2), roadmap, changelog, README, threat model T65–T66;
      `scripts/build-kali.sh` installs `chosproxy-cli` into the .deb
      (not tried here: needs Docker). All checks green (682 Rust tests,
      177 vitest).

## Previous milestone: M27 — API / GraphQL Workspace (done, on main)

Branch: none — the user wants everything on `main` from now on (no feature
branches; commit each step to `main`). Matrix rows: SCANNER-014 (API
scanning: OpenAPI, Postman, SOAP), SCANNER-015 (GraphQL), and the
deferrals into M27: EDITOR-001 (Pretty-view editing), EDITOR-003
(multipart/XML Inspector editing), TARGET-003 (per-node parameter list
and inline request list in the site map).

**Scope decision (keep it):** like M25/M26, no attack payloads. Importing
a definition only reads a file. Sending its requests is the tester's
choice, scope-bound and through the Request Engine (pools, sessions);
by default only safe methods (GET, HEAD, OPTIONS) are sent in bulk, and
other methods need an explicit switch. GraphQL introspection is one
standard read query sent when the tester asks for it.

Plan (tick and commit as steps land):

- [x] **A. API definitions import** — done: `crates/apispec`
      (`parse(text, base_url)`, `request_text`, `is_safe_method`;
      `openapi.rs` v3 + v2 with local `$ref`, security placeholders;
      `postman.rs` folders, `{{vars}}`, raw/urlencoded/formdata/graphql;
      `wsdl.rs` quick-xml tree, SOAP 1.1/1.2 envelopes from the schema;
      `example.rs` schema examples; YAML via `yaml-rust2`). Original plan: new crate
      `crates/apispec` parsing OpenAPI 3.x and Swagger 2.0 (JSON and
      YAML, via a safe-Rust YAML parser), Postman collections v2.1 and
      WSDL 1.1 (SOAP 1.1/1.2, with `quick-xml`) into endpoints: method,
      URL from servers and paths (path parameters filled with examples),
      query, headers, a body from examples or schemas (JSON, form,
      multipart, SOAP envelope). Bounded (file size, endpoints, schema
      depth, `$ref` cycles).
- [x] **B. API workspace** — done: schema v23 `api_definitions`
      (`crates/storage/src/apidefs.rs`); `engine/api_ops.rs`
      (`api_import`, `api_import_text`, `api_definitions`,
      `api_definition` with hosts, `api_definition_delete`, `api_request`,
      `api_send_start` task kind `apiSend`: ticked hosts only, scope,
      safe methods unless `includeUnsafe`, rate limit, `session_send`
      tool `crawler`, source `scanner`, history note); background passive
      analysis now includes source `scanner`; test `crates/app/tests/api.rs`;
      robustness seeds; Tauri `api_*` commands; UI Target → API workspace
      (`features/api/ApiView.tsx`). Original plan: definitions
      kept in the project (schema v23), the endpoint list with filters,
      "Send to Repeater / Fuzzer", "Send endpoints" as a task (safe
      methods by default, scope-checked, rate-limited, through pools and
      session rules) whose traffic the passive analyzer and scans see.
- [x] **C. GraphQL workspace** — done: `crates/apispec/src/graphql.rs`
      (`INTROSPECTION_QUERY`, `parse_introspection`, `build_operation`,
      `is_graphql_request`, `request_body`); `ApiEndpoint.read_only`
      (GraphQL queries read-only, mutations not); schema v24 column
      `api_definitions.schema`; `engine/graphql_ops.rs`
      (`graphql_introspect(exchange)` = one Repeater-like scoped send,
      schema → API definition of format `graphql`, `graphql_import_schema`
      from a file, `graphql_schema`, `graphql_build`, `graphql_request`);
      lab `POST /graphql`; tests in `tests/api.rs`; request menu "Get
      GraphQL schema (introspection)"; `features/api/GraphqlPanel.tsx`;
      message viewer "GraphQL" tab (`lib/graphql.ts`). Original plan: detection of GraphQL
      requests in the history; an introspection request on demand; the
      schema parsed into queries, mutations and types; a query builder
      (fields, arguments, variables) that sends through Repeater; a
      GraphQL tab in the message viewer (query and variables pretty).
- [x] **D. Editing structured bodies** — done: `lib/inspector.ts`
      (`multipartFields` with ranges, file parts `fixed`; `xmlFields`
      element text and attributes; `applyRangedFields` replaces only the
      changed values, XML escaped; Content-Length follows), editable in the
      Inspector; Repeater "Format body" / "Compact" (`reformatBody` in
      `lib/httpText.ts`) = editing the body in pretty form. Original plan: Inspector
      editing of multipart fields and XML values using the M25
      insertion-point parsers (`replace` with the context encoding);
      Pretty-view editing of JSON bodies.
- [x] **E. Site map per-node details** — done: `engine/sitenode_ops.rs`
      (`site_node_details(SiteFilter)`: parameters from the latest 200
      requests via insertion points, latest 50 requests), Tauri
      `site_node_details`, site map panel tabs Parameters / Requests
      (click opens Repeater, right-click the request menu). Original plan: parameter names per
      node (from insertion points) and the node's requests inline.
- [x] **F. Docs and checks** — done: ADR-0037, matrix (64/50/18/2),
      roadmap, changelog, README, threat model T62–T64. All checks green
      (673 Rust tests, 176 vitest).

## Previous milestone: M26 — Resource Pools & Live Tasks (on main)

Done; its branch was merged into `main` and deleted. Matrix rows: INTRUDER-006 (resource
pools), REPEATER-001 / PLATFORM-001 (shared pools; resume after reopening),
SCANNER-009 (live tasks), SCANNER-001 (multi-request analysis), SCANNER-003
(JS analysis in the background analyzer), SCANNER-006 (scan dashboard,
per-request crawl reasons), SCANNER-007 (per-check selection in the UI),
EXT-002 (plugin interception hooks).

**Scope decision (keep it):** like M25, nothing here sends attack payloads.
Live tasks are *passive* (live passive crawl, live passive audit); there is
no live active audit (that is M13, pending). Plugin hooks only act on
traffic the tester already sends through the proxy, need a new permission
granted explicitly, are time-bounded and fail open (the request goes on).

Plan (tick and commit as steps land):

- [x] **A. Resource pools** — done: `Limiter::with_delay`, `back_off`,
      counters (`crates/request/src/limiter.rs`); `engine/pool_ops.rs`
      (`PoolsConfig` in `pools.toml`, `PoolRuntime`, `pool_enter` in
      `session_send` and the Repeater kept connections); lab `/busy`; test
      `crates/app/tests/pools.rs`; Tauri `resource_pools` /
      `save_resource_pools`; UI Crawler & tasks → Resource pools…
      (`ResourcePoolsDialog.tsx`). Original plan: named pools `{id, name, concurrency,
      requests per second, random delay, back off on 429/503 (Retry-After,
      bounded)}` in `pools.toml` plus an assignment per tool (crawler and
      scans, fuzzer, sequencer, repeater, plugins, discovery). Shared
      `Limiter`s acquired in `session_send` (every tool send goes through
      it); the task's own limits still apply. Live counters per pool (in
      flight, waiting, sent, throttled). No assignment = old behaviour.
      Tauri commands + Crawler & tasks → Resource pools.
- [x] **B. Background JS analysis and multi-request analysis** — done:
      `crates/scanner/src/cross.rs` (`CrossAnalyzer`, `CROSS_CHECKS`:
      `cross-session-not-renewed`, `cross-referer-leak`,
      `cross-inconsistent-headers`); `passive_ops::{all_checks,
      cross_issues}`, `Inner.passive_cross` (reset on open/re-analysis);
      the analyzer runs JS checks too; scans feed every item to a
      `CrossAnalyzer`; test `crates/app/tests/live.rs`. Original plan: the
      background analyzer also runs the JavaScript checks; a stateful
      cross-request analyzer (`crates/scanner/src/cross.rs`) fed message by
      message: session token not rotated at login, token-bearing URL leaked
      in a Referer to another host, security headers set on some HTML
      responses of a host but not others. Used by the analyzer, scans and
      live tasks.
- [x] **C. Live tasks** — done: `engine/live_ops.rs` (`LiveTaskConfig`,
      `LiveMode` audit/crawl, `live_task_start`, `resume_live_tasks` on
      open, cursor kept in the config as `resumeAfter`); `Control::Suspend`
      and `run_task` in `task_ops.rs` (live tasks survive close/open);
      storage `task_set_config`, `tasks_live_unfinished`, `exchange_urls`,
      `discovery_mark_requested` (live crawl hits reuse `discovery_results`,
      status 0 = not requested); tests in `tests/live.rs`; UI Crawler &
      tasks → Live task… (`LiveTaskDialog.tsx`, `CheckPicker.tsx`).
      Original plan: task kind `live` (live passive audit / live
      passive crawl): sources, in-scope only, URL filter, check selection;
      follows new traffic until stopped; live crawl lists links seen in
      responses that were never requested. Live tasks running when the
      project closes are paused and restarted when it reopens.
- [x] **D. Scan dashboard, crawl reasons, per-check selection** — done:
      crawler `Entry.via` + `Frontier::push_via`, `task_ops::crawl_reason`
      (crawl exchanges are stored first, notes "Crawler: a link on #N");
      migration v22 `tasks.summary`, storage `task_set_summary` /
      `task_summary`; `ScanSummary`/`ScanPhase`/`PhaseState`,
      `scan_summary`; UI `ScanDashboard.tsx` in the task panel,
      `CheckPicker` in the scan dialog. Original plan: the crawl
      records why each URL was requested (start URL, link/form/script on
      #id, redirect, robots/sitemap) in the exchange notes; scans save a
      structured summary (phase, counters per phase, findings by severity)
      shown in a dashboard in Crawler & tasks; the scan dialog edits the
      check selection with checkboxes.
- [x] **E. Plugin interception hooks (EXT-002)** — done: proxy
      `hook.rs` (`RequestHook`, `HookVerdict`, `set_request_hook`) and the
      hook step in `relay.rs`; plugin `Permission::ProxyIntercept`,
      `PluginMessage::{RegisterHook, HookResult}`, `HostMessage::Hook`,
      `HookRequest`; host `engine/plugin_hooks.rs` (`PluginHooks`, scope
      required, 2 s timeout, notes); test
      `plugin_hooks_change_block_and_answer_in_scope_requests`; PLUGIN_SDK.md.
      HTTP/2 clients are not hooked (→ M31). Original plan: permission
      `proxy.intercept` (API v2): `registerHook` → `hook {call, request}`
      → `hookResult {continue | replace | block | respond}`; applied in the
      HTTP/1 relay after Match & Replace and before Intercept, in-scope
      traffic only, 2 s timeout fails open, recorded in the history notes
      and flags; HTTP/2 relay → later if it does not fit.
- [x] **F. Docs and checks** — done: ADR-0036, matrix (62/50/20/2),
      roadmap, changelog, README, threat model. Reviewed afterwards: the
      limiter held its mutex while sleeping (pool status and back-off calls
      blocked up to 60 s) and leaked the waiting counter on cancellation —
      fixed with regression tests in `limiter.rs`. 640 Rust tests.

## Previous milestone: M25 — Advanced Scanner Engine (merged)

Merged into `main` (the branch is deleted). Matrix rows: SCANNER-003 (JS analysis),
-004 (insertion points), -005 (phases), -006 (scan debugging views), -007
and SETTINGS-003 (configuration library), -008 (launcher), -010 (dedup),
-012 (false positives), -016 (scheduled scans), INTRUDER-003 (date
generators), INSTR-001 (runtime instrumentation).

**Scope decision (keep it):** M25 builds the scanner *infrastructure*
and passive analysis only. A scan here is crawl → passive audit →
JavaScript analysis, scope-bound and rate-limited. Active checks belong
to M13, which remains pending and was selected for implementation on
2026-11-14. Insertion points are used by the Fuzzer
and will be reused by M13.

Plan (tick and commit as steps land):

- [x] **A. Insertion points** — done: `crates/scanner/src/insertion.rs`
      (`insertion_points(text, kinds)`, `replace`, `encode`, `mark`;
      `PointKind`, `PointEncoding`); Fuzzer auto-mark now uses it
      (`fuzz_auto_mark(text, kinds)`, "Auto §" + "▾" menu); the old
      `chosproxy_fuzzer::auto_mark` is gone. Original plan: a parser that lists the
      insertion points of a request (path segments, query and form
      parameters, cookies, JSON fields at any depth, multipart fields, XML
      text, chosen headers, whole body) with byte ranges and the base value,
      and builds a request with one value replaced (encoding kept). Used by
      a Fuzzer "Auto-mark positions" button.
- [x] **B. Fingerprints and dedup** — done: `crates/scanner/src/fingerprint.rs`
      (`fold_path`, `url_fingerprint`, `request_fingerprint`); crawler
      `CrawlConfig.max_similar` (default 20, 0 = off), `Push::Similar`,
      `Frontier::with_max_similar`, logged in the crawl task. Original plan: a structural request
      fingerprint (method, host, path with ids/UUIDs/hashes folded, sorted
      parameter names). The crawler can skip structural duplicates; the scan
      uses it so each item is audited once.
- [x] **C. Scan configurations, launcher, phases, event log** — done:
      `engine/scan_ops.rs` (`ScanConfig`, `ScanLaunch`, `ScansConfig` in
      `scans.toml`, 4 built-ins, save/delete/import/export, `scan_start`,
      task kind `scan`: crawl via `run_crawl`, then audit of each request
      fingerprint once with "Audited/Skipped … why" log lines;
      `findings_record_auto`; storage `task_progress_note`); test
      `crates/app/tests/scan.rs`; Tauri commands `scan_configs`,
      `save_scan_config`, `delete_scan_config`, `export_scan_configs`,
      `import_scan_configs`, `scan_start`; UI Crawler & tasks → New scan…
      (`features/tasks/ScanDialog.tsx`). Original plan:
      (SCANNER-005/006/007/008, SETTINGS-003, CRAWL-003 presets): named JSON
      configurations (built-in, none aggressive, plus user ones, import and
      export); a "New scan…" launcher (start URLs in scope, configuration);
      task kind `scan` running phases crawl → passive audit → JS analysis;
      a scan event log that says *why* each request was made or skipped.
- [x] **D. Passive JavaScript analysis** — done:
      `crates/scanner/src/javascript.rs` (checks `js-endpoints`,
      `js-source-map`, `js-secrets` (masked), `js-dom-sinks`); runs in the
      scan's JavaScript phase (not in the background analyzer yet); lab
      `/app.js` has a never-called sink and a sourceMappingURL. Original plan: on JS responses,
      endpoints and URLs found in code, source-map references, hard-coded
      secrets patterns, dangerous DOM sinks; findings with evidence.
- [x] **E. False positives and suppressions** — done: migration v21
      `finding_suppressions`, `crates/storage/src/suppressions.rs`
      (`SuppressionRule`, `NewSuppression`, host `*.example` patterns, path
      prefix; checked in `record_one` so passive, scan and plugin findings
      are skipped, `PassiveRecorded.suppressed`); engine
      `suppressions`/`suppression_add(rule, mark_open)`/`suppression_delete`;
      UI: finding editor "Suppress similar…", Findings bar "Suppressions".
      The falsePositive status itself existed already. Original plan: mark a
      finding as false positive (with a note), suppression rules (check +
      host/path) that hide future matches, managed in the Findings view.
- [x] **F. Date payload generator and scheduled scans** — done: Fuzzer
      `PayloadSource::Dates {from, to, stepDays, format}` (own calendar
      code, tokens yyyy/yy/MM/M/MMM/dd/d) and its editor; schedule kind
      `scan` (config = `ScanLaunch` JSON) in storage, `schedule_ops.rs`,
      `ScheduleControls` and the scan dialog; test in `tests/scan.rs`.
- [x] **G. Docs and checks** — done: ADR-0035, matrix (57/52/23/2),
      roadmap (row ✅ + "Milestone 25 (done)" with Deferred), changelog,
      README, threat model T55–T58. All checks green (611 Rust tests, 157
      vitest).

Deferred from the start (to be listed in the ROADMAP "Deferred" section):
runtime instrumentation INSTR-001 (needs a design decision; → M31),
checks that send requests and the active audit (M13, pending), raw and
streaming HTTP/2 (→ M31), OAST SMTP/ACME and OAST providers via plugins
(→ M28), Pretty-view editing (→ M27, with the API workspace parsers).

## Previous milestone: M24 — Authenticated Browser Crawling (merged)

(M23 is merged; its notes are in ADR-0033 and ROADMAP.)

Matrix rows: SESSION-001…007 (sessions & auth), PROXY-017 (platform
auth), CRAWL-002 (browser crawler), TARGET-007 (crawl paths). Existing
pieces to reuse:
- the project cookie jar (`crates/app/src/cookies.rs`, `cookie_jar_*` in
  storage);
- the DOM Inspector's CDP client (`crates/dom/src/cdp.rs`) for driving
  Chromium;
- the crawler (`crates/crawler`, engine `task_ops.rs`).

Plan (tick and commit as steps land):

- [x] **A. Keyring** (SESSION-007) — done: `crates/app/src/keyring.rs`,
      `engine/keyring_ops.rs` (`credentials`, `credential_set`,
      `credential_delete`, internal `credential(id)`), Tauri commands,
      Settings → Credentials. `reload_platform_auth()` is a stub for B.: `crates/app/src/keyring.rs`. Entries
      `{id, label, username, secret, hosts}`, encrypted at rest with
      AES-256-GCM (ring) in `credentials.enc`, key in `credentials.key`
      (0600). Secrets never logged, exported or sent to the UI after
      saving (only "set" and "delete"). OS keychain integration → M31.
- [x] **B. Platform authentication** (PROXY-017) — done:
      `NetworkConfig.auth`, `crates/request/src/auth.rs` (Basic, Digest
      MD5/SHA-256 matching RFC 7616 vectors; every rule for the host is
      tried), one retry in `send()`, `Sent.authenticated`, history note,
      `reload_platform_auth()`; lab `/auth/basic` and `/auth/digest`
      (user lab / secret); test `crates/app/tests/platform_auth.rs`; UI in
      Settings → Network. Original plan:: rules `{host pattern,
      basic|digest, credential id}` in `network.toml`. The Request Engine
      answers a 401 challenge once with Basic or Digest (RFC 7616, MD5 and
      SHA-256, qop=auth) for tool sends (Repeater, Fuzzer, crawler,
      plugins). Proxy traffic: the browser handles its own prompts. NTLM →
      deferred.
- [x] **C. Session rules and login macros** — engine done and tested
      (`engine/session_ops.rs`: `SessionRule`, `session_send` used by
      Repeater/Fuzzer/Sequencer/crawler/plugins, `{{var}}` and
      `{{cred:id.username|secret}}`, macro + retry; lab `/session/*`; test
      `crates/app/tests/sessions.rs`). `Sent.sent_request` + `Sent.notes`
      replaced `Sent.authenticated`. Tauri commands `session_rules` /
      `save_session_rules`; UI: Project → Session rules
      (`features/project/SessionRulesCard.tsx`). (SESSION-001/002/005/006):
      project-level rules `{name, hosts, tools, use cookie jar, validity
      check (status / regex in the response), macro (history ids replayed
      in order, cookies into the jar, `{{var}}` extracted by regex), headers
      to set from variables}`, applied around tool sends; on a failed
      validity check, run the macro and retry once.
- [x] **D. Browser-powered crawling** — engine done and tested with real
      Chrome (`crates/dom/src/driver.rs`, `engine/browser_crawl_ops.rs`:
      `browser_crawl_start`, task kind `browserCrawl`; `launch_lab_browser`
      split out of `launch_browser_with`; lab `/js-links` → `/js-only*`;
      test `crates/app/tests/browser_crawl.rs`, skipped without Chromium).
      Tauri command `browser_crawl_start`; UI Crawler & tasks → Browser crawl…
      (`features/tasks/BrowserCrawlDialog.tsx`). Original plan: (CRAWL-002, SESSION-004): drive
      Chromium over CDP through the proxy (traffic captured, source
      crawler), breadth-first over in-scope links read from the rendered
      DOM, with a page limit; a "recorded login" = a login macro run first
      (or the tester logs in in the lab browser before the crawl).
- [x] **E. Docs and checks.** ADR-0034, matrix (53/47/32), roadmap,
      changelog, README, threat model T52–T54. Note: one unit test run of
      `chosproxy-app --lib` failed once and did not reproduce in 9 later
      runs (likely the timing-sensitive browser-session test); watch it.

## Previous milestone: M23 — Extension Platform / ChosHub (merged)

Scope, from ROADMAP, the matrix (EXT rows) and the deferrals of M14/M18:

- A stable, versioned Extension API (v2 alongside v1), documented.
- New permissions, each granted explicitly:
  - `network.send`: send requests through the Request Engine, scope-checked;
  - `project.read`: read stored traffic on request;
  - `ui.extend`: extension-provided actions and menu items;
- external OAST providers through plugins, using `network.send`.
- Signed distribution: packages signed with a developer key, verified
  before install; a local catalog ("ChosHub") that installs from a
  directory/file. No network marketplace without an explicit decision.

Findings so far: API v1 is "separate process, JSON lines on stdin/stdout"
(`crates/plugin/src/protocol.rs`: `HostMessage` Hello/Exchange/Shutdown,
`PluginMessage` Ready/Log/Finding/Annotate). Permissions live in
`crates/plugin/src/permissions.rs`: `network.send`, `project.read` and
`ui.extend` exist but `supported()` is false. The host side is
`crates/app/src/engine/plugin_ops.rs`. Grants and the approved file hash
live in `plugins.toml`. The example plugin is in `plugins/`.

Plan (steps; tick them and commit as they land):

- [x] Study the plugin system (above).
- [x] **A. API v2 calls** — protocol done and committed (`crates/plugin`:
      `API_VERSION = 2`, `SUPPORTED_APIS`, `Permission::supported_in`,
      `PluginMessage::{Send, Query, RegisterAction, Output}`,
      `HostMessage::{Result, Action}`); host side done in `plugin_ops.rs`
      (`plugin_send`, `plugin_query`, `plugin_actions`, `plugin_run_action`,
      `SendBudget`; plugin traffic is not fed back). End-to-end test
      `api_v2_plugins_send_query_and_add_actions` passes. Tauri commands
      `plugin_actions` / `plugin_run_action`; plugin actions appear in every
      request menu ("Plugin actions") and show their output in the script
      action dialog. HelloPlugin stays on API 1 (shows v1 still works). (manifest `api = 2`; v1 plugins keep working).
      Plugin → host messages carry an `id`, and the host answers with
      `result {id, ok, data | error}`:
  - `send` (`network.send`): a raw request plus a target, sent through the
    Request Engine, scope-checked like the Fuzzer (refused out of scope
    when a scope exists), rate-limited, stored in the history with source
    `plugin`;
  - `query` (`project.read`): one exchange by id, a history page, or
    findings;
  - `registerAction` (`ui.extend`): a label shown in every request menu;
    the click sends `action {action, exchange}` to the plugin, and its
    `output {action, text}` is shown in the dialog used by action scripts.
- [x] **B. Signed packages / local ChosHub.** Done so far:
      `crates/plugin/src/signing.rs` (`sign_dir`, `verify_dir`, `Verified`,
      `generate_key`, `key_fingerprint`; `plugin.sig` excluded from
      `hash_dir`) and the `chosplugin` CLI (keygen/sign/verify, tried by
      hand). Engine done and tested
      (`signed_plugins_install_from_a_catalog_and_require_trust`):
      `PluginsConfig.trusted` / `require_signed`, `PluginInfo.signature`,
      `plugin_trust`, `plugins_require_signed`, `plugins_install_from`.
      Tauri commands + Plugins view UI done (signature badge, "Trust this
      publisher", signed-only switch, "Install from folder…"). Original plan: Ed25519 (ring) signature
      `plugin.sig` over the plugin fingerprint; trusted publisher keys in
      `plugins.toml`; Plugins view shows signed/unsigned/untrusted; a small
      `chosplugin` CLI (`keygen`, `sign`, `verify`). Install from a folder
      (catalog = a local directory of plugin folders). No network store.
- [x] **C. Docs and checks:** ADR-0033, PLUGIN_SDK.md (v2), threat model,
      matrix EXT-001/EXT-003, roadmap, changelog; the HelloPlugin example
      updated to v2.
- Deferred, explicitly (decided in C, see ADR-0033): EXT-002 interception
  hooks → M26; external OAST providers through plugins → M25; OS sandbox
  for plugin processes → M31; no hosted marketplace without a decision.
