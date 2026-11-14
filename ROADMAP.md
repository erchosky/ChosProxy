# Roadmap

Sources: [docs/PROJECT_SPEC.md](docs/PROJECT_SPEC.md), the
[parity addendum](docs/PROJECT_SPEC_ADDENDUM_PARITY.md) and the
[capability parity matrix](docs/BURP_PARITY_MATRIX.md) (every matrix row is
assigned to one of the milestones below).

Each milestone must be usable on its own. A milestone is **done** only when:
it compiles, works, has tests, does not break earlier milestones, is
documented, handles important errors, its UI has loading/error/empty states,
has no fake controls, and `lint` + CI pass. Capabilities additionally follow
the parity rule (addendum §127) before their matrix row turns 🟢.

> **Picking the work up?** Read [STATUS.md](STATUS.md) first: where the
> project stands, the milestone in progress, how work is done and checked,
> and the engineering and product requirements.

Status legend: ✅ done · 🚧 in progress · ⏳ planned

## Product tiers

| Tier | Contents | Target |
|---|---|---|
| **Core** | Proxy, Intercept, History, Repeater, Site Map, WebSockets, Decoder, Comparer, Organizer | 1.0 (M17) |
| **Pro** | Crawler, Scanner, Fuzzer, Sequencer, OAST, DOM Inspector, Content Discovery, authenticated crawling, Live Tasks, Resource Pools, ChosChecks, advanced reporting, extension platform | M18–M28, parity audited at M32 (not declared) |
| **Next** | ChosAI, ChosAgent, visual request flow, timelines, automatic response diffing, local-first AI, educational explanations | M29–M30 and continuous |

1.0 is the stable **Core** release. Before tagging it, a feature discovery
audit (addendum §124) re-checks the matrix: every Core row must be 🟢 or
explicitly deferred with a reason. The M32 audit did not declare parity (see docs/audits/2026-11-14-m32-parity-audit.md).

## Milestones

| # | Milestone | Status | Exit criterion |
|---|---|---|---|
| 0 | Foundation | ✅ | Workspace, Rust core, Tauri + React shell, SQLite + migrations, settings, logging, tests, CI |
| 1 | HTTP proxy | ✅ | Real HTTP traffic captured, stored, browsable with request/response viewers |
| 2 | HTTPS | ✅ | Per-install CA, CONNECT + TLS interception, leaf certs, CA export/regeneration, TLS passthrough rules, lab browser launcher |
| 3 | Intercept | ✅ | Pause, edit, forward, drop requests and responses; interception rules |
| 4 | Repeater + Request Engine | ✅ | Shared Request Engine (ADR-0009) used by Repeater; tabs, groups, raw editing, send/cancel, timing, per-tab history and diff, cookie jar, shared context menu |
| 5 | Target | ✅ | Scope engine, site map, FilterEngine, global search, Inspector, Logger, annotations, listener auth |
| 6 | Utilities | ✅ | Decoder (chains, smart decode), Comparer (incl. structured HTTP diff), Organizer, sandboxed Render view |
| 7 | WebSockets | ✅ | Handshake + frame capture, interception, search, export |
| 8 | Projects | ✅ | Full persistence, Save As, export, automatic backups, project settings layer, settings import/export |
| 9 | Passive scanner | ✅ | Engine + first checks; findings with evidence and educational explanations |
| 10 | Crawler + Task system | ✅ | Scope-bound crawling; central task system with a first resource-pool limiter (concurrency + rate) |
| 11 | Fuzzer | ✅ | Positions, attack modes, payload generators, grep match/extract, results view |
| 12 | Sequencer | ✅ | Token capture and statistics with visual explanations |
| 13 | Active audit framework | ✅ | Disabled by default; requires explicit target, non-empty scope, per-launch authorization confirmation and rate/request budgets; reflected XSS (`Tentative`), SQLi (boolean + error with negative controls), open redirect, CORS, and correlated ChosOAST checks; custom active check scripts (`SCANNER-013`) and API endpoint active audit (`SCANNER-014`) (ADR-0044) |
| 14 | Plugins | ✅ | Permissioned SDK v1, isolation, HelloPlugin |
| 15 | Reporting | ✅ | HTML / JSON / Markdown reports (PDF later) |
| 16 | Performance & hardening | ✅ | Profiling, parser fuzzing, security review, keybindings, onboarding |
| 17 | 1.0 (Core) | ✅ | Feature discovery audit of Core rows; stable `.deb` + AppImage for Kali x86_64 |
| 18 | OAST / ChosOAST | ✅ | Local and self-hosted out-of-band interaction server, correlation to requests and findings |
| 19 | DOM Inspector | ✅ | Source → sink analysis in the lab browser |
| 20 | Engagement Tools | ✅ | Find References, Target Analyzer, Content Discovery, CSRF PoC, scheduler, FrameLab, site map snapshots/diff (traffic simulator: not applicable, ADR-0028) |
| 20b | Editor & workflow conveniences | ✅ | Editor menus and copy as code, JSON field editing in the Inspector, saved filters, custom key bindings, Repeater notes, palette history search, automatic redirects, scope list import (rest explicitly deferred) |
| 21 | Advanced Proxy & Network | ✅ | Upstream HTTP/SOCKS5 proxies per destination, host name overrides, configurable timeouts, invisible listeners, Match & Replace (HTTP/2 → M21b, network auth → M24, ADR-0029) |
| 21b | HTTP/2 in the Request Engine | ✅ | HTTP/2 for Repeater and every tool (ALPN h2 / h2c), single-packet attack for parallel groups, CA import (ADR-0030) |
| 21c | HTTP/2 through the proxy & TLS options | ✅ | HTTP/2 between browsers, proxy and servers (per listener), invisible "redirect to", port-specific passthrough (ADR-0031); raw HTTP/2 and OAST SMTP/ACME → M25 |
| 22 | ChosChecks / ChosScripts | ✅ | Sandboxed scripts (Rhai, ADR-0032): checks, history filters and columns, actions, rewrite rules, library with tests |
| 22b | Conditions & scripted tools | ✅ | History condition builder, regex Intercept/passthrough rules, WebSocket Match & Replace, script import/export, Repeater "Run script", Fuzzer payload scripts |
| 23 | Extension Platform / ChosHub | ✅ | Plugin API v2 (send, query, menu actions) alongside v1, Ed25519-signed plugins, trusted publishers, local catalog install (ADR-0033) |
| 24 | Authenticated Browser Crawling | ✅ | Encrypted keyring, platform auth (Basic/Digest), session rules with login macros and variables, browser-powered crawler (ADR-0034) |
| 25 | Advanced Scanner Engine | ✅ | Insertion points, structural dedup, phased scans (crawl → passive audit → JavaScript analysis) with an explained event log, configuration library, launcher and scheduling, suppression rules (ADR-0035); no active checks (M13) |
| 26 | Resource Pools & Live Tasks | ✅ | Resource pools shared across tools with back-off, live passive audit/crawl that resume with the project, cross-request checks, JavaScript analysis in the background, scan dashboard and crawl reasons, per-check selection, plugin request hooks (ADR-0036); no live active audit (M13) |
| 27 | API / GraphQL Workspace | ✅ | OpenAPI/Swagger/Postman/WSDL import, API workspace with bounded sends, GraphQL introspection, schema browser and operation builder, multipart/XML Inspector editing, site map node parameters (ADR-0037); no active audit (M13) |
| 28 | REST API / Headless | ✅ | Opt-in loopback automation API with a token (no browser or DNS-rebinding access, audited), `chosproxy-cli serve` headless mode and client commands (ADR-0038) |
| 29 | ChosAI | ✅ | Explanations, finding analysis, Repeater prompts and a login helper from a local model (Ollama, LM Studio, llama.cpp) or an opt-in external provider; redaction and an engine-enforced context preview (ADR-0039) |
| 30 | ChosAgent | ✅ | Agent tasks over a fixed catalog of ChosProxy tools with engine-enforced DENY/ASK/ALLOW (sending always asked, scoped, budgeted), plan, queued messages, approvals, AI activity log; ChosAI conversations and reviewable session rules (ADR-0040); no active audit (M13) |
| 31 | Hardening / Recovery / Encryption | ✅ | Encrypted projects (SQLCipher), temporary file management, Performance view (ADR-0041); crash recovery with instance-isolated session markers and read-only mode (step A), encrypted Organizer/settings sharing UI and IPC (step C), Security Center (§120) and complete audit log coverage with filters (§122, step F) (ADR-0044) |
| 31b | Deferral sweep | ✅ | The items earlier milestones parked on M31 (list in "Milestone 31b") |
| 32 | Parity audit | ✅ | Every row checked against the code and the tests (docs/audits/2026-11-14-m32-parity-audit.md). **Parity is not declared**: 4 rows are not built (active audit, crash recovery, runtime instrumentation, extension editor tabs) and 41 are partial |
| 33 | Small gaps | ✅ | Collapsed Repeater groups; unstored out-of-scope proxy traffic; Fuzzer stop conditions, CSV export, result colours and columns; event log kept across restarts; resource pool in launch dialogs; a test for editor search (ADR-0043) |

## Dependencies between modules

```
Request Engine (M4) ─┬─► Repeater (M4) ─► Fuzzer (M11) ─► Scanner (M13/M25)
                     ├─► Crawler (M10) ─► Browser crawler (M24)
                     ├─► Content Discovery (M20)
                     ├─► Plugins (M14/M23), ChosAgent (M30)
                     └─► Upstream/SOCKS/HTTP2 (M21) plug in here, once
Scope engine (M5) ──► every automated tool, Live Tasks, Scheduler, Agent
FilterEngine (M5) ──► History, Logger, Site Map, WebSockets, Scanner, Organizer
Task system (M10) ─► Crawler, Fuzzer, Scanner, Discovery, imports/exports, Agent tasks
Resource pools (M10 basic, M26 full) ─► every automated sender
Permission model (M14) ─► Plugins, ChosScripts (M22), REST API (M28), ChosAgent (M30)
Findings store (M9) ─► Scanner, ChosChecks, OAST, DOM Inspector, plugins, reporting
Lab browser (M2) ─► DOM Inspector (M19), browser crawler (M24), recorded logins
Settings layers (M8) ─► configuration library (M25), import/export
```

## Milestone 0 — Foundation (done)

- Cargo workspace + pnpm workspace, shared lint/format config.
- `chosproxy-core`: branding, typed config sections in separate TOML files,
  paths, events, redaction, errors.
- `chosproxy-storage`: project SQLite DB, versioned migrations, writer thread.
- Tauri 2 desktop shell: sidebar layout, dark/light/system themes, command
  palette (Ctrl/Cmd+K) with real actions only, settings screen, status bar.
- Structured logging (`tracing`) with redaction and log-level setting.
- Unit tests, TS type generation from Rust (`ts-rs`), CI workflow.

## Milestone 1 — HTTP proxy (done)

- Multiple listeners (default `127.0.0.1:8080`), start/stop/restart,
  interface + port selection, remote connections off by default.
- Wire-exact HTTP/1.1 forwarding: absolute-form requests, keep-alive,
  chunked, `Expect: 100-continue`, 1xx, HEAD/204/304, close-delimited bodies,
  upgrade pass-through, loop protection, 502/504 synthesized errors.
- `CONNECT` tunnelled opaquely (no decryption until M2).
- History stored in SQLite with all spec §5 fields; capture size cap with
  truncation flag.
- History table (virtualized, paged) with text/method/status filters, and a
  request/response viewer with Raw / Pretty / Hex.
- Local lab server (`chosproxy-lab`) and integration tests driving real
  traffic through the proxy.
- Kali x86_64 `.deb` and AppImage built by `scripts/build-kali.sh` (Kali
  container) and by CI.

## Milestone 20 — Engagement Tools (done)

- Content discovery (`chosproxy-discovery`, ADR-0028): built-in, site-map
  and custom names, extensions, well-known files, recursion, soft-404
  fingerprints, scope per request, limits; only hits stored; schema v16.
- CSRF PoC generator and FrameLab (clickjacking verdict + overlay PoC),
  reviewable HTML that ChosProxy never serves.
- Target analyzer (parameters, comments, scripts) and "Find references".
- Site map snapshots and comparison (schema v17).
- Scheduler for crawls and discovery while the project is open (schema v18).
- "Simulate manual testing traffic": not implemented (➖, ADR-0028).

### Moved to M20b
- The editor and workflow conveniences deferred by the 1.0 audit (below).

## Milestone 31b — Deferral sweep (done)

- ChosAI: answers are streamed and can be stopped; "Record login" hands
  the requests of a login made in the browser to the login-flow helper.
- ChosAgent: a task that stopped (or was interrupted when the project
  closed) is continued by a new task that starts from its activity log.
- Automation API: `GET /v1/openapi.json`, `GET /v1/events` (Server-Sent
  Events), `chosproxy-cli events` and `openapi`.
- Plugins: hooks for HTTP/2 clients and for responses (same permission).
- A resource pool chosen for one running task.
- A certificate per listener: its own CA, or one chosen certificate.
- NTLMv2 platform authentication for tool sends.
- Keyring key in the macOS Keychain or behind a master password.
- ChosOAST accepts mail (SMTP sink), off by default.
- Crawl graph; Organizer nested collections with drag and drop; Repeater
  tabs dragged and coloured; conditions in the site map; Format body and a
  GraphQL editor in Intercept; a settings library for the crawler, content
  discovery and the Fuzzer.
- Rewrite scripts for WebSocket text messages.

### Deferred (each needs a decision; see STATUS.md)
- Runtime instrumentation (INSTR-001): the design is still open.
- ACME certificates for a self-hosted OAST server; model and OAST
  providers through plugins (they talk to outside services).
- Raw and streaming HTTP/2 frames in Repeater (its own milestone).
- GraphQL-specific checks; other payload generators; replaying recorded
  browser interactions as logins; script data-flow analysis.
- Secret Service on Linux for the keyring key (a D-Bus dependency in the
  Kali build); Kerberos / Negotiate; NTLM towards an upstream proxy.
- Choosing a resource pool in the launch dialogs; collapsing a Repeater
  group (both done in M33); scope-based TLS pass-through rules.
- Selected next from M31 (2026-11-14): crash recovery (A), the user interface of
  encrypted sharing (C), Security Center and audit coverage (F).
- The plugin OS sandbox (not started).

## Milestone 33 — Small gaps (done)

- Repeater: groups collapse to their chip (saved in the project,
  schema v28).
- Target → Scope: "Don't store out-of-scope proxy traffic" (forwarded,
  not stored).
- Fuzzer: stop conditions (matches, requests without a response, status
  codes); CSV export of every result; highlight colours on results
  (schema v29); hideable result columns.
- The event log is kept across restarts (private file, last 2,000).
- A resource pool can be chosen when a crawl, scan, discovery, Fuzzer
  attack or API send is launched.
- A test for search in the message editor.

### Deferred
- PDF reports (a renderer would be a new dependency; HTML reports print
  to PDF from a browser) — needs a decision.
- Dropping out-of-scope proxy traffic (by decision, ADR-0043: it is only
  left unstored).
- Everything listed under "Deferred" in Milestone 31b that is not marked
  done. M31 steps A, C, F and M13 are selected next by the user
  (2026-11-14); implementation is pending.

## Milestone 13 — Active Audit Framework (done)

- Typed active check contract (`crates/scanner/src/active.rs`, ADR-0044):
  stable ID, issue metadata, compatible insertion point kinds, per-point
  request budget, pure probe planning (`plan_point_probes`,
  `plan_request_probes`) and deterministic baseline-vs-probe evaluation
  (`evaluate_point_probes`, `evaluate_request_probes`) separated from
  network I/O.
- Built-in non-destructive active check catalog:
  - `active-reflected-xss`: innocuous marker and context probes; reports
    `Confidence::Tentative` (since no browser execution is performed).
  - `active-sqli`: controlled boolean comparisons (`OR 1=1` vs `AND 1=2`)
    plus a broken-quote negative syntax control and corroborated SQL error
    signatures, suppressing dynamic pages and generic 500s; never extracts
    data or modifies state.
  - `active-open-redirect`: inspects `Location` headers for external probe
    targets without following redirects.
  - `active-cors`: probes `Origin` reflection and `null` origin allowance
    with `Access-Control-Allow-Credentials: true` against the baseline.
  - `active-oast`: injects correlated ChosOAST URLs when OAST is enabled
    and links interactions back to both baseline and probe exchanges.
- Custom active check scripts (`SCANNER-013`, `fn active_probes(value)` +
  `fn active_check(baseline, probe, payload)`) and API endpoint active
  scanning (`SCANNER-014`, "Audit endpoints…" in the API workspace).
- Engine active audit phase (`crates/app/src/engine/scan_ops.rs`): off by
  default; requires explicit target URLs, a non-empty project scope,
  per-launch user authorization confirmation (`active_authorized` stripped
  when saving/importing templates), and rate/request budgets; excludes
  non-idempotent methods unless explicitly enabled; shares Resource Pools
  and Session Rules; revalidates scope on every probe; records dual
  evidence (baseline + probe exchange, `SCANNER-011`) and `Source::Scanner`
  history entries.

### Deferred from M13 (explicitly)
- Broader multi-variant active check families (command injection, SSTI,
  XXE, deserialization, GraphQL-specific active checks) and browser-verified
  active XSS execution (`SCANNER-002` remains 🟡).
- Live active audit on background proxy traffic (intentionally omitted so
  active probing only runs inside explicitly launched, budgeted scan tasks).

## Milestone 31 — Hardening / Recovery / Encryption (done)

- Encrypted project files (SQLCipher): password on create and open,
  encrypted temporary projects, change password, Save As / export with or
  without encryption, encrypted backups; CLI and API passwords without
  the command line (docs/PROJECT_FILES.md).
- Crash recovery (`PROJECT-004`, spec §118): per-instance session markers in
  `sessions/` (`0600`, no secrets) with PID + process start-time liveness
  checks so sibling instances are never stolen; startup recovery dialog
  offering Restore (`PRAGMA quick_check`, password prompt for encrypted
  projects, interrupted traffic-sending tasks marked `Cancelled`, live
  passive tasks resumed), Open read-only (enforced across the engine with a
  read-only banner), or Discard (with optional cleanup of orphaned
  temporary projects; recoverable temp files protected from automatic
  cleanup).
- Encrypted sharing (`ORGANIZER-003`): atomic `0600` exports, nested
  `parent/...` collection prefix matching, format inspection (`inspect_export_file`)
  before prompting for a password, and full Tauri IPC + UI in Organizer and
  Settings → Transfer.
- Security Center (§120) and audit log coverage (§122, `PLATFORM-002`):
  local-only `security_overview()` with 13 posture checks and navigation
  links in Settings → Security & Privacy; complete project audit log events
  (tasks, active scans, plugin permissions, ChosAgent approvals/rejections,
  crash recovery, encryption/rekey, exports, scope/finding changes) with
  prefix/category, text search and date-range filtering in Project → Audit log.
- Settings → Storage: ChosProxy's temporary files, a safe "Clean now",
  old leftovers of other runs only on request; cleanup at exit.
- Tools → Performance: memory, CPU, database size, storage rate, proxy,
  pools and tasks, local only.

### Deferred from M31 (explicitly)
- OS-level process sandboxing for external plugins (`Seatbelt` / `seccomp` /
  `Landlock`).
- Secret Service (D-Bus) keyring backend on Linux.

### Moved to M31b (explicitly)
Everything earlier milestones deferred to M31 that is not hardening:
streaming AI answers; recording a login in the lab browser (AI-004);
agent tasks surviving a project reopen; model and OAST providers through
plugins; OAST SMTP/ACME; an OpenAPI description and event streaming for
the automation API; GraphQL-specific checks and Intercept pretty editing;
script data-flow analysis; WebSocket payload processing; raw and
streaming HTTP/2 and HTTP/2 plugin hooks; the plugin OS sandbox; OS
keychain integration; NTLM; INSTR-001; the crawl graph (TARGET-007); a
configuration library for other tools (SETTINGS-003); Organizer nested
collections and drag and drop; a certificate per listener; a global pool
choice.

## Milestone 30 — ChosAgent (done)

- ChosAgent (sidebar): give a goal, the agent works with ChosProxy's own
  tools — history, requests, findings, scope, passive checks, Decoder,
  notes, tentative findings, session rules, and sending a request.
- Settings → ChosAgent: Deny / Ask / Allow per tool, enforced by the
  engine; sending is always Ask, needs a scope and has a per-task budget.
  Approvals show the exact request and the agent's reason.
- Plan, messages to a running agent, pause / resume / stop; with an
  external model every context is approved first.
- AI activity log of every ChosAI question and agent step; the agent's
  requests are in HTTP history and the Logger (source `agent`).
- ChosAI: follow-up questions; the login-flow rule opens in Project →
  Session rules for review.

### Deferred from M30 (explicitly)
- Streaming answers → M31 (the Request Engine reads whole responses).
- Recording a login in the lab browser for AI-004 → M31.
- Agent tasks surviving a project reopen → M31.
- Model providers through plugins → M31 (from M29).
- Agents over the automation API: not planned without a decision.
- Active checks or payload lists for the agent: no (M13 remains pending).

## Milestone 29 — ChosAI (done)

- Settings → ChosAI: off, local model (loopback only) or external provider
  (opt-in, https, key in the keyring).
- "Explain with ChosAI…" on requests, findings, Repeater and selections:
  explanations, manual checks, fixes, a login-flow helper with a suggested
  session rule, report text.
- Only attached items are sent, redacted and limited; a preview shows the
  exact text, and external models only get a previewed context.
- ChosAI writes text only.

### Deferred from M29 (explicitly)
- Streaming answers, conversations, AI activity log, applying suggested
  session rules, ChosAgent → M30.
- Model providers through plugins → M31.

## Milestone 28 — REST API / Headless (done)

- Automation API (Settings → Automation API, or `chosproxy-cli serve`):
  JSON under `/v1` on 127.0.0.1, token required, browser requests and
  foreign `Host` headers refused, changes audited (docs/API.md).
- `chosproxy-cli`: headless `serve`, and client commands for status,
  history, findings, scope, sends, crawls, scans, tasks, reports and API
  definitions; in the Kali package.

### Deferred from M28 (explicitly)
- OAST SMTP/ACME and OAST providers via plugins → M31.
- An OpenAPI description of the API and event streaming over it → M31.

## Milestone 27 — API / GraphQL Workspace (done)

- Target → API workspace: import OpenAPI 3, Swagger 2 (JSON/YAML), Postman
  v2 and WSDL 1.1; endpoints with example requests open in Repeater or the
  Fuzzer; "Send endpoints" sends them to ticked hosts only (scope, safe
  methods unless asked, rate limit, pools, session rules).
- GraphQL: "Get GraphQL schema" on a request runs one introspection; the
  schema becomes a definition (queries read-only); schema browser and
  operation builder; GraphQL tab in the message viewer.
- Inspector: multipart and XML values editable; Repeater "Format body" /
  "Compact".
- Site map: parameters and latest requests per node.
- Schema v23–v24; new crate `chosproxy-apispec`.

### Deferred from M27 (explicitly)
- Active audit of API endpoints → M13 (selected next, implementation pending).
- GraphQL-specific checks and pretty editing in Intercept → M31.
- Remote `$ref` and imports: not fetched by design.

## Milestone 26 — Resource Pools & Live Tasks (done)

- Resource pools (Crawler & tasks → Resource pools…): requests in flight,
  rate, random delay, back-off on 429/503, assigned per tool (crawler,
  Fuzzer, Sequencer, Repeater, plugins); live counters.
- Live tasks (Crawler & tasks → Live task…): live passive audit with its
  own checks, live passive crawl listing URLs nobody requested; sources,
  scope and URL filters; they resume when the project is reopened. They
  never send requests.
- Cross-request checks: session not renewed at login, tokens leaked in the
  Referer, security headers missing on some pages. The background analyzer
  also runs the JavaScript checks.
- Scans: a dashboard (phases, counters, issues by severity), a note on
  every crawl request saying why it was made, per-check selection.
- Plugins (API v2, `proxy.intercept`): request hooks that let in-scope
  proxy requests go on, change, block or answer them, within 2 s, failing
  open; schema v22.

### Deferred from M26 (explicitly)
- Live active audit → M13 (selected next, implementation pending).
- Hooks for HTTP/2 clients and for responses, per-task pool choice,
  data-flow analysis of scripts → M31.

## Milestone 25 — Advanced Scanner Engine (done)

- Insertion points (path, query, form, cookies, headers, JSON, multipart,
  XML) with context encoding; the Fuzzer's "Auto §" uses them, with a
  choice of kinds.
- Structural fingerprints: the crawler limits similar URLs per pattern;
  scans audit each pattern once.
- Scans (Crawler & tasks → New scan…): a configuration from the library
  (4 built-ins, saved ones, JSON import/export), phases crawl → passive
  audit → JavaScript analysis, an event log that explains each decision,
  scheduling.
- JavaScript analysis: endpoints, source maps, secret-looking values
  (masked), DOM sinks.
- Suppression rules for false positives (Findings → Suppressions, "Suppress
  similar…").
- Fuzzer: date payloads.
- No attack payloads are sent: the active audit stays with M13.

### Deferred from M25 (explicitly)
- Active audit and checks that send requests → M13 (selected next, implementation pending).
- Runtime instrumentation (INSTR-001), raw and streaming HTTP/2, other
  payload generators → M31.
- JavaScript analysis in the background analyzer, a scan dashboard,
  per-check selection in the scan UI, multi-request analysis → M26.
- Pretty-view editing, multipart/XML Inspector editing → M27.
- OAST SMTP/ACME and OAST providers via plugins → M31 (moved from M28, ADR-0038).

## Milestone 24 — Authenticated Browser Crawling (done)

- Keyring: encrypted credentials (Settings → Credentials), write-only
  secrets.
- Platform authentication: Basic and Digest (MD5/SHA-256) per host for
  tool sends (Settings → Network).
- Session rules (Project → Session rules): cookie jar, headers with
  `{{var}}` / `{{cred:…}}`, expiry checks, login macros from stored
  requests, one retry; used by Repeater, Fuzzer, Sequencer, crawler and
  plugins.
- Browser crawl (Crawler & tasks → Browser crawl…): the lab browser
  renders pages through the proxy and follows script-made links.
- Lab: `/auth/basic`, `/auth/digest`, `/session/*`, `/js-links`.

### Deferred from M24 (explicitly)
- NTLM/Kerberos, OS keychain and a keyring master password, replaying
  browser interactions as recorded logins, the crawl graph view → M31.

## Milestone 23 — Extension Platform / ChosHub (done)

- Plugin API v2 next to v1 (ADR-0033): `send` (`network.send`, Request
  Engine, scope- and rate-limited, stored with source `plugin`), `query`
  (`project.read`: exchange, history, findings), menu actions
  (`ui.extend`), calls answered by `result`; refused calls are answered,
  logged and audited.
- Signed plugins: `plugin.sig` (Ed25519 over id, version and file
  fingerprint), `chosplugin` keygen/sign/verify, trusted publishers and a
  signed-only switch in Plugins, install from a catalog folder.

### Deferred from M23 (explicitly)
- Plugins blocking or answering live traffic (EXT-002) → M26.
- External OAST providers through plugins → M25.
- OS-level sandbox for plugin processes → M31.
- A hosted marketplace: not planned without a separate decision.

## Milestone 22b — Conditions & scripted tools (done)

- HTTP history / Logger → Conditions: a condition builder (URL, method,
  status, MIME, request/response headers and bodies, length; contains, is,
  matches regex, is present/missing, >, <; all or any) run as a filter
  script in the sandbox; "Save as filter script".
- Regular expressions in Intercept rules (URL) and HTTPS passthrough
  (`host:port`).
- Match & Replace for text WebSocket messages, each direction, before
  Intercept.
- Scripts: import/export as JSON (imported scripts arrive switched off),
  payload scripts as a Fuzzer processing step, "Run script" in Repeater.

### Deferred from M22b (explicitly)
- Scope-based passthrough rules, conditions in the site map tree, scripts
  for WebSocket messages → M31; date and other payload generators → M25.

## Milestone 22 — ChosChecks / ChosScripts (done)

- `chosproxy-script` crate (ADR-0032): Rhai in a sandbox (no files,
  network or processes; operation, size and depth limits; no `eval` or
  modules), a read-only message API with regex and Base64 helpers.
- Five kinds: checks (findings in passive analysis), filters and columns
  (HTTP history, Logger), actions (every request menu, output dialog) and
  rewrite scripts (Match & Replace "Request/Response script").
- Tools → Scripts: library in `scripts.toml`, templates, compile problems
  with lines, test runs on stored exchanges; guide in docs/SCRIPTS.md.

### Deferred from M22 (explicitly)
- Filter condition builder, regex Intercept and passthrough rules,
  WebSocket messages in Match & Replace, script import/export, an action
  button in Repeater, scripted Fuzzer payload processing → M22b.
- Checks that send requests → M25.

## Milestone 21c — HTTP/2 through the proxy & TLS options (done)

- Per listener "HTTP/2": intercepted HTTPS offers ALPN `h2`; each HTTP/2
  stream is read whole, shown as `HTTP/2` text, changed by Match & Replace,
  paused by Intercept and captured; servers get HTTP/2 over one shared
  connection, or HTTP/1.1 when they do not offer it (ADR-0031).
- Invisible listeners: optional "redirect to" `host:port` for everything.
- Passthrough patterns with a port (`host:8443`).
- Lab server: an HTTPS mode that offers only HTTP/1.1.

### Deferred from M21c (explicitly)
- Streaming HTTP/2 bodies (over 64 MB), raw/invalid HTTP/2, OAST SMTP and
  ACME certificates → M25.
- A certificate per listener → M31; regex and scope-based passthrough
  rules → M22.

## Milestone 21b — HTTP/2 in the Request Engine (done)

- A request whose line says `HTTP/2` is sent over HTTP/2 by every sender
  (Repeater, Fuzzer, Sequencer, crawler): TLS with ALPN `h2` or cleartext
  h2c, through the network settings of M21. Messages keep the text form
  (`HTTP/2 200`) in the history, viewers and analysis (ADR-0030).
- Repeater → Options → Protocol switches a tab between HTTP/1.1 and HTTP/2.
- Parallel groups of HTTP/2 tabs to one server use the single-packet
  attack.
- Proxy → HTTPS → Import CA… (validated; the previous CA is kept).
- Lab server answers HTTP/2 (`/version` shows the protocol).

### Deferred from M21b (explicitly)
- HTTP/2 between browsers, the proxy and servers; raw or invalid HTTP/2;
  per-listener TLS options; a fixed "redirect to host" for invisible
  listeners; port/regex passthrough rules; OAST SMTP and ACME → M21c.

## Milestone 21 — Advanced Proxy & Network (done)

- Settings → Network (`network.toml`, ADR-0029), applied in the Request
  Engine so the proxy, Repeater and every tool follow it:
  - upstream proxy rules per destination (HTTP with CONNECT or absolute
    form, SOCKS5 with remote or local DNS, Basic / user-password
    credentials, Direct to exempt hosts);
  - host name overrides (exact or `*.domain` → IPv4/IPv6);
  - connect, response and idle timeouts.
- Invisible listeners: Host header for plain HTTP, server name (SNI) for
  TLS on a configurable port; a loop guard refuses requests that land back
  on the proxy.
- Match & Replace (Intercept → Match & Replace): request line, headers and
  body, status line, response headers and body; literal or regex; bodies
  decoded first; originals kept in the history.
- Repeater redirect policy saved with the tab (schema v20).

### Deferred from M21 (explicitly)
- HTTP/2 end to end, single-packet attack, per-listener TLS options, a
  fixed "redirect to host" for invisible listeners, port/regex passthrough
  rules, custom CA import, OAST SMTP and ACME → M21b.
- Match & Replace for WebSocket messages and scripted rules, regex
  Intercept rules → M22.
- Platform authentication to destination hosts and NTLM/Digest proxy
  authentication → M24 (credential keyring).

## Milestone 20b — Editor & workflow conveniences (done)

- Command palette: typing three or more characters also finds stored
  requests (URL, headers, bodies) and opens them in HTTP history or the
  Logger.
- Scope: paste a list of hosts, `*.domain` entries and URLs to add them as
  include or exclude rules (comments and duplicates skipped).
- Repeater: automatic redirect following per tab (never, on-site, in scope,
  always; at most 10 hops, out-of-scope hops refused); a note per tab saved
  with the project (schema v19).
- Message editors: right-click / Shift+F10 menu with copy, decode, "Search
  the project for it", and copy as curl, Python (requests) or JavaScript
  (fetch).
- Inspector: JSON body field values editable in Intercept and Repeater; the
  body and Content-Length are rewritten.
- HTTP history: named saved filters (settings `ui.savedFilters`).
- Settings → Keyboard shortcuts: rebind or clear every global shortcut and
  view shortcut, conflicts moved with a notice (settings `ui.keyBindings`).

### Deferred from M20b (explicitly)
- Editing in the Pretty view, editing multipart fields and XML bodies in the
  Inspector → M25 (they need the same structured body parsers as scanner
  insertion points).
- A condition builder for filters → M22 (with ChosScripts filters).
- Per-node parameter list and inline request list in the site map → M27
  (endpoint and parameter views of the API workspace).
- Drag and drop of Repeater groups and Organizer collections, nested
  collections, highlight colours on Repeater tabs → M31 (final UI pass).
- Saving the redirect policy with the Repeater tab → M21 (done in M21).

## Milestone 19 — DOM Inspector (done)

- `chosproxy-dom` crate (ADR-0027): instrumentation installed in every
  frame of the lab browser before page scripts via the DevTools Protocol
  (Chromium family), reporting through a binding; sources → HTML/JS/URL
  sinks traced with a session canary and by source values; navigations,
  postMessage listeners, prototype pollution, DOM clobbering.
- Project schema v15: observations merged by kind, sink, source, page and
  code location; findings (source `dom`) when the page response is in the
  history.
- DOM Inspector view: start, canary helpers, navigation of the instrumented
  tab, Source → Transformation → Sink with value, location and stack.
- Lab: intentionally vulnerable pages under `/dom/`; guide in
  docs/DOM_INSPECTOR.md.

### Deferred from M19 (explicitly)
- Firefox (WebDriver BiDi) → M24 with the browser-powered crawler.
- Prototype-pollution gadget scanning and automatic source injection → M25.

## Milestone 18 — OAST / ChosOAST (done)

- `chosproxy-oast` crate (ADR-0026): identifiers with a checksum, an
  authoritative zone-only DNS responder (no recursion, rate-limited, no
  amplification), HTTP/HTTPS listeners with a fixed response, and a
  token-protected, certificate-pinned polling API.
- Modes (off by default): **Local** (server inside ChosProxy, loopback by
  default, optional DNS) and **Self-hosted** (`chosoast` binary on a machine
  you control with a delegated zone; guide in docs/OAST.md).
- Project schema v14: payloads, interactions (only for the project's own
  identifiers), links to the requests that carried them; "External service
  interaction" findings (source `oast`) with the request as evidence.
- OAST view (payloads, live interactions, raw data, carrying requests),
  "New OAST payload" in the command palette, OAST filter in Findings.

### Deferred from M18 (explicitly)
- External OAST providers through plugins → M23 (needs `network.send`).
- SMTP and other protocols; ACME certificates for HTTPS interactions → M21 (moved to M21b).
- Automatic payload insertion by the active scanner → M25.

## Milestone 17 — 1.0 Core (done)

- Feature discovery audit of every Core row
  ([docs/audits/2026-11-11-m17-core-audit.md](docs/audits/2026-11-11-m17-core-audit.md)):
  each one implemented, or partial/planned with a named milestone.
- Gaps closed: history column sorting and MIME filter, copy as curl,
  Inspector body fields and response cookies, WebSocket messages in global
  search, hex selection to the Decoder, site map context menu.
- Patched bundled SQLite (RUSTSEC-2026-0459); version 1.0.0; `.deb` and
  AppImage for Kali x86_64 built in a Kali container and smoke-tested.

### Deferred from M17 (explicitly)
- Advanced network handling (HTTP/2, upstream/SOCKS, Match & Replace,
  invisible proxying, configurable timeouts, regex interception rules)
  → M21.
- Editor and workflow conveniences (Pretty editing, menus inside editors,
  JSON field editing, annotations outside history, saved filters, custom
  key bindings, automatic redirect following) → M20.

## Milestone 16 — Performance & hardening (done)

- Parser robustness: a deterministic mutation harness
  (`crates/app/tests/robustness.rs`) feeds corrupted variants of realistic
  messages to every parser of untrusted input (HTTP heads, framing, bodies,
  content codings, cookies, URLs, WebSocket close frames, passive checks,
  link discovery, token extraction, fuzzer templates, plugin messages and
  manifests, Decoder operations): no panics, no input over 2 s, ≈600 000
  inputs in a long run.
- Security review of M4 and M9–M14 (THREAT_MODEL.md T3, T10, T11, T16,
  T24–T28): the Fuzzer and Sequencer refuse out-of-scope targets when a
  scope is defined, as the crawler does.
- Keyboard shortcuts with a `?` overlay and a palette command; the
  sidebar fits short windows; "Getting started" includes the scope.
- Performance: analysis throughput measured (docs/PERFORMANCE.md); Fuzzer
  result rows re-render only when their selection changes.

### Deferred from M16 (explicitly)
- User-defined key bindings → M20.
- WebSocket messages in the global search, Decoder for selected hex bytes,
  Inspector for JSON/XML/multipart bodies → done in M17 (XML → M20).

## Milestone 15 — Reporting (done)

- `chosproxy-report` crate: one report model rendered as a self-contained
  HTML page, Markdown or JSON (`chosproxy-report-v1`). Sections: executive
  summary with severity counts, target hosts and scope rules, methodology,
  findings overview, then each finding with severity, confidence, status,
  description, remediation, technical details and evidence.
- Report view (Target → Report): title, client, author, summary and
  methodology; choose findings (false positives left out by default);
  evidence on/off, credential redaction (on by default) and a size limit per
  message; live sandboxed preview; export to a chosen file.
- Every exported report is recorded in the project audit log (ADR-0018).

### Deferred from M15 (explicitly)
- PDF output (print the HTML report to PDF meanwhile), custom templates and
  branding, XML output.

## Milestone 14 — Plugins (done)

Done (ADR-0025, PLUGIN_SDK.md):
- `chosproxy-plugin`: manifest (`plugin.toml`), permission vocabulary,
  JSON-lines protocol (API v1).
- Plugin host: plugins run as separate processes while a project is open,
  with a minimal environment, receive stored traffic (`network.read`), add
  findings (`scanner.passive`) and annotate history (`project.write`);
  every message is checked against the grants; refusals are logged and
  audited; approval tied to a fingerprint of the plugin's files.
- Plugins view (install the example, permissions, enable/disable, status,
  log); HelloPlugin example; lab `/debug-token`.

### Deferred from M14 (explicitly)
- `network.send`, `project.read`, `ui.extend`; WASM-sandboxed scripts
  (M22); signed distribution (M23).

## Milestone 12 — Sequencer (done)

Done (ADR-0024):
- `chosproxy-sequencer`: token locations (cookie, header, regex) with
  suggestions from a sample response; analysis of length, character set
  and encoding, per-position entropy, bit balance, monobit and chi-square
  tests, repeats, fixed parts and sequential tokens, as explained
  observations without a single verdict.
- Captures run as tasks (schema v13 for tokens, traffic source
  `sequencer`); tokens can also be pasted.
- Sequencer view with summary tiles, observations and charts (validated
  chart colors per theme), "Send to Sequencer" in the request menu; lab
  endpoints `/token` and `/token/sequential`.

### Deferred from M12 (explicitly)
- Further statistical batteries (FIPS runs/poker, spectral, correlation),
  time-stamp decoding, token export.

## Milestone 11 — Fuzzer (done)

Done (ADR-0023):
- `chosproxy-fuzzer`: `§…§` positions with auto-marking, payload sets
  (your list or a file, numbers, character combinations, built-in test
  lists — no credentials), processing rules (prefix, suffix, replace,
  case, Decoder encodings and hashes), sniper / battering ram / pitchfork /
  cluster bomb plans, grep match and extract.
- Attacks run as tasks (pause, resume, stop, retries, concurrency and rate
  limits), with a baseline request; results stored per request (schema
  v12), sortable and filterable (matched, status differs from baseline),
  with the request and response of each row.
- Fuzzer view (attack setup and results), "Send to Fuzzer" in the request
  menu, "Fuzz" in Repeater, "Open Fuzzer" in the command palette. Fuzzer
  traffic goes through passive analysis.

### Deferred from M11 (explicitly)
- Scripted payload processing (M22), stop conditions and recursive grep,
  shared resource pools (M26).

## Milestone 10 — Crawler and tasks (done)

Done (ADR-0022):
- Task system: tasks and their logs saved in the project (schema v11),
  pause / resume / stop, progress, requests, errors and duration; tasks
  left running when a project closes are recorded as cancelled.
- `Limiter` in the Request Engine: requests in flight and requests per
  second per task.
- `chosproxy-crawler`: validated crawl settings, link discovery (links,
  frames, scripts, forms, redirects, JavaScript URLs, robots.txt,
  sitemaps) and a breadth-first frontier with depth, variant and
  static-file limits and words to avoid.
- Crawls run GET only, in scope (or on the start sites without a scope),
  never submit POST forms, can use the cookie jar, store traffic as
  `crawler` (Logger, site map, passive analysis).
- "Crawler & tasks" view, "New crawl" dialog, "Crawl from this URL" in the
  request menu and the command palette.

### Deferred from M10 (explicitly)
- Browser-powered and authenticated crawling (M24); shared resource pools
  and live tasks (M26); resuming tasks after reopening a project; crawl
  presets.

## Milestone 9 — Passive scanner (done)

Done:
- Findings store (schema v7, ADR-0017): type, severity, confidence, review
  status, location, description, remediation, notes and several pieces of
  evidence copied from captured requests; "Finding" action on requests;
  Findings view with filters; counts on the dashboard; audit log entries.
- Performance pass (docs/PERFORMANCE.md): parallel batched global search
  (4× faster), visible-only throttled refreshes, bounded history memory,
  on-demand views (startup script 865 kB → 265 kB), fixed WebSocket and
  event-log refreshes under continuous traffic.

- Passive analysis (ADR-0021): `chosproxy-scanner` with 18 checks, each
  with an explanation and remediation; a background analyzer that follows
  the history (schema v10 cursor), merges repeated detections into one
  finding with up to five pieces of evidence, respects false positives and
  accepted risks and reopens fixed findings that come back; settings
  (on/off, scope, Repeater traffic, per-check switches); status and
  "Analyze all again" in the Findings view.

### Deferred from M9 (explicitly)
- Checks across several exchanges, JavaScript analysis (M25); plugin
  checks (M14); a public-suffix list for cookie checks.

## Milestone 8 — Projects (done)

- Save As (temporary → file), export a copy (optionally in-scope only),
  both as consistent `VACUUM INTO` copies that never overwrite (ADR-0016).
- Automatic backups per project: interval, number kept, in-scope only,
  folder; verified before older ones are rotated; restore opens a copy.
- Settings layers: scope "Use global / Override for this project"; project
  backup settings and Markdown notes (schema v6).
- Settings import/export as JSON with per-section validation and a diff
  preview; credentials never exported.
- Append-only project audit log; Organizer collections.

### Deferred from M8 (explicitly)

- Project overrides for settings other than the scope → when needed.
- Encrypted projects, crash recovery → M31.
- Repeater tabs, findings and tasks in the project → M4, M9, M10.

## Milestone 7 — WebSockets (done)

- `crates/websocket`: RFC 6455 frame codec keeping exact wire bytes,
  fragment assembly, close parsing (ADR-0015).
- Proxy relays WebSockets frame by frame after the `101`, captures every
  message and removes handshake extensions so payloads stay readable.
- Intercept for WebSocket messages per direction (host and scope rules):
  edit text, forward unchanged, drop; originals stored.
- Proxy → WebSockets: connections (linked to the handshake), messages with
  direction/opcode filters, payload search (text or regex), detail with
  pretty JSON, Decode and Compare, JSON export. Schema v5.
- Lab server `/ws` echo endpoint and `/ws-demo` page for manual tests.

### Deferred from M7 (explicitly)

- Manually re-sending WebSocket messages → Repeater (M4).
- Displaying `permessage-deflate` traffic (extensions are removed instead).
- WebSocket messages in the global search → done in M17.
- WebSockets over HTTP/2 → M21.

## Milestone 6 — Utilities (done)

- Decoder (`crates/decoder`): encodings, compression, JSON, JWT
  inspection and hashes as editable chains with per-step output; smart
  decode proposes a chain; "Decode selection" from every message viewer
  and the Intercept editor.
- Comparer: line, word and byte diffs plus a structured HTTP comparison
  (status, headers, query, cookies, form, sizes, body); "Compare" from
  history, Logger, Search and Intercept.
- Organizer: items copied from history with status, severity, tags and
  notes, persisted in the project (schema v4).
- Render view for HTML responses in a fully sandboxed frame (ADR-0007,
  ADR-0014).

### Deferred from M6 (explicitly)

- Comparer items persisted in the project → M8 if wanted.
- "Send to Repeater" → M4.
- Decoder for selected bytes in the hex view → done in M17.

## Milestone 5 — Target (done)

- Scope engine (`core::scope`, `scope.toml`): include/exclude rules by
  protocol, host (+subdomains or regex), port and path; URL tester; "Add to
  scope" / "Exclude" from the site map and history; scope always visible in
  the top bar (ADR-0013).
- Shared filter model compiled to SQL (history, site map, search): URL text
  or regex, in scope only, source tool, annotated, site map node.
- Site map built from all traffic, with marks; global search over URLs,
  headers, decoded bodies and notes; highlights and notes on history
  entries; Intercept "only in-scope targets" (schema v3).
- Inspector: query/form parameters, cookies and headers, read-only in every
  request viewer and editable in Intercept (only changed parts rewritten).
- Logger (all tools' traffic, follow mode); HTTP history limited to proxy
  traffic.
- Proxy listener authentication (HTTP Basic, salted PBKDF2 hash, 407,
  credentials never forwarded).
- Event log of the session (listeners, projects, scope, CA, notices).

### Deferred from M5 (explicitly)

- Inspector for JSON/XML/multipart bodies and response cookies → JSON, multipart and response cookies in M17; XML → M20.
- Event log persistence → project audit log (M8).
- Digest/other proxy authentication schemes → M21 if needed.
- Repeater actions from the site map/history ("send to") → M4.

## Milestone 4 — Repeater (done)

Done (ADR-0019, ADR-0020):
- The Request Engine's send path in `chosproxy-request`: the upstream
  connector shared with the proxy, requests written byte for byte
  (malformed ones too), responses read with the wire-exact codec, size cap,
  timeouts, timing split into DNS / connect / TLS / server / download.
- Repeater tabs saved in the project (schema v8): new, rename, duplicate,
  reorder, close; target field; raw editor with Inspector; Send
  (Ctrl/Cmd+Enter) and Cancel; Update Content-Length switch.
- Per-tab history (back/forward, list, restore a past request), every send
  stored as `repeater` traffic in the Logger, "Diff previous" in the
  Comparer, "Follow redirect".
- "Repeater" action in HTTP history, Logger, Search, Organizer and
  Intercept (a paused request, edits included); "Open Repeater" in the
  command palette.

- Tab groups (schema v9) and group sends: in sequence, in sequence over a
  single connection, in parallel with last-byte synchronisation.
- `Connection` in the send path; "Reuse the connection" per tab.
- Project cookie jar filled from proxy and Repeater responses; "Use the
  cookie jar" per tab.
- Shared context menu for "send to" actions (history, Logger, Search,
  Organizer) and for Repeater tabs and groups.

### Deferred from M4 (explicitly)
- HTTP/2 and the single-packet attack, upstream proxies (M21); resource
  pools (M26); session-handling rules and macros (M24); drag and drop of
  tabs and collapsing groups; "copy as curl".

## Milestone 3 — Intercept (done)

- Intercept switch (top bar, view, palette), always off at start; rules
  for requests/responses, hosts in/out, methods, parameters, static assets
  and response MIME, saved in `proxy.toml`.
- Paused messages (bounded queue) shown with wait time; editor with
  byte-exact text round-trip, Content-Length update, hex view for binary,
  Unpack for chunked/compressed bodies; Forward (Ctrl/Cmd+Enter), Forward
  & intercept response, Drop (explanatory 502), Forward all, copy raw/URL/
  headers. Turning Intercept off releases everything unmodified.
- History marks edited/dropped exchanges and shows the original (schema v2).
- ADR-0012; tests: queue, rules, text round-trip, proxy end-to-end, engine,
  UI store; manual E2E in the desktop app.

### Deferred from M3 (explicitly)

- "Send to Repeater/Comparer/Organizer" and "Decode selection" actions →
  M4/M6 when those tools exist.
- ~~Scope-based intercept rules ("only in-scope") → M5.~~ Done in M5.
- Pretty (JSON/form) editing and Inspector edits → M4/M5.
- WebSocket message interception → M7.

## Milestone 2 — HTTPS (done)

- `chosproxy-tls`: per-installation local CA (generated on first start,
  key `0600`, validated on load, never overwritten implicitly), per-host
  leaf certificates, upstream TLS with system + Mozilla roots (ADR-0011).
- CONNECT interception decided by the first client bytes: TLS terminated
  with a certificate for the SNI, plain HTTP captured, other protocols
  tunnelled. Lazy upstream connection with readable error pages.
- Policy: interception switch, passthrough hosts, per-host verification
  exceptions (default loopback), noted on affected exchanges.
- CA export (PEM/DER), confirmed regeneration, download at
  `http://chosproxy/cert`, client TLS failures counted and reported.
- Lab browser: installed Chromium-family (SPKI pin, no CA import needed) or
  Firefox (profile-local CA import via certutil), temporary isolated
  profile deleted on exit.
- HTTPS lab server (`--tls-port`), end-to-end tests with real TLS clients,
  manual E2E with Google Chrome on macOS.

### Deferred from M2 (explicitly)

- HTTP/2 interception (ALPN offers HTTP/1.1 only) → M21 with ADR-0009.
- "Set scope from current URL" in the lab browser → M5.
- Bundled browser (we use the installed Chromium/Firefox) → re-evaluate M16.
- Graphical Kali check of the lab browser with Kali's Chromium.

### Deferred from M1 (explicitly)

- ~~Proxy authentication~~ done in M5; hostname/protocol rules → M21.
- History notes editing and per-column sorting → M5.
- ~~"Render" view~~ done in M6 (sandboxed renderer).
- The desktop entry category (`03-webapp-analysis`) must be confirmed on a
  graphical Kali install.

## Beyond the plan

HTTP/3, gRPC, mobile proxying, collaborative projects: architecture keeps
room for them (version-agnostic message model, UI-agnostic Engine), no
milestone yet. New capabilities are added to the spec and matrix first.
