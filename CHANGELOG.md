# Changelog

All notable changes are documented here. Format: Keep a Changelog; versions
follow SemVer once 1.0 ships.

## [Unreleased]

### Fixed — macOS packaging (final release check, 2026-11-14)
- `scripts/make-macos-dmg.sh`: clears iCloud extended attributes from the
  staged app before signing; `codesign` failed with "resource fork, Finder
  information, or similar detritus not allowed" when the repo lives in
  iCloud-synced `~/Documents`.
- `scripts/make-backup.sh` (mac): puts the stripped, ad-hoc-signed `.app`
  from the DMG in the backup instead of the unsigned one from
  `target/release/bundle/macos`, and unmounts the DMG on failure.

### Added — Milestone 13 (Active audit framework) and Milestone 31 completion (steps A, C, F)
- **Active audit framework (M13)**: optional active scan phase (disabled by
  default; requires explicit target URLs, a non-empty project scope,
  per-launch user authorization confirmation, and rate/request budgets;
  excludes non-idempotent methods by default; shares Resource Pools and
  Session Rules):
  - `active-reflected-xss`: innocuous marker and context probes; reports
    `Tentative` confidence (no browser execution performed).
  - `active-sqli`: controlled boolean differential (`OR 1=1` vs `AND 1=2`)
    with a broken-quote negative syntax control and corroborated SQL error
    signatures (suppresses dynamic pages and generic 500s; never extracts
    data or alters state).
  - `active-open-redirect`: inspects `Location` headers for external probe
    targets without following redirects.
  - `active-cors`: probes `Origin` reflection and `null` origin allowance
    with credentials against the baseline.
  - `active-oast`: injects correlated ChosOAST URLs when OAST is enabled.
  - Custom active check scripts (`fn active_probes(value)` +
    `fn active_check(baseline, probe, payload)` in `ScriptKind::Check`,
    `SCANNER-013`).
  - API workspace "Audit endpoints…" button launching a passive or active
    scan pre-filled with the API definition's target URLs (`SCANNER-014`).
  - Active findings link both the baseline exchange and the proving probe
    exchange as evidence (`SCANNER-011`).
- **Crash recovery (M31 A, `PROJECT-004`, spec §118)**: per-instance session
  markers in `sessions/` (atomic `0600`, no secrets) with PID + process
  start-time liveness checks; startup recovery dialog offering Restore
  (`PRAGMA quick_check`, password prompt on encrypted projects, interrupted
  traffic-sending tasks marked `Cancelled`, live passive tasks resumed),
  Open read-only (enforced across the engine with a visible read-only
  banner), or Discard (with optional cleanup of orphaned temporary projects).
- **Encrypted sharing UI and IPC (M31 C, `ORGANIZER-003`)**: atomic `0600`
  exports, nested `parent/...` collection prefix matching, format inspection
  (`inspect_export_file`) before prompting for a password, and full export /
  import controls in the Organizer and Settings → Transfer.
- **Security Center & project audit log coverage (M31 F, spec §120 & §122,
  `PLATFORM-002`)**: local-only `security_overview()` with 13 posture checks
  and one-click navigation in Settings → Security & Privacy; complete audit
  log coverage (tasks, active scans, plugin permissions, ChosAgent
  approvals/rejections, crash recovery, encryption/rekey, exports,
  scope/finding changes) with category/prefix, text search and date-range
  filtering in Project → Audit log.

### Milestone 33 (Small gaps)
- Repeater: collapse a group to its chip (saved in the project).
- Target → Scope: "Don't store out-of-scope proxy traffic"; the traffic is
  still forwarded.
- Fuzzer: stop conditions (after N matches, after N requests without a
  response, on status codes); "Export CSV" of every result; highlight
  colours on results; a Columns menu to hide result columns.
- The event log is kept across restarts (the last 2,000 entries; earlier
  sessions are dimmed).
- Crawl, scan, discovery, Fuzzer and API send: choose the resource pool
  when launching.
- Schema v29.

### Milestone 32 (Parity audit) and the quality pass before it
- Findings → Issue definitions: what each automatic check reports, why it
  matters and how to fix it.
- The parity matrix was checked row by row; several statuses were wrong
  and are corrected. Parity is not declared (docs/audits/).
- Fixed: the automation API kept serving a connection that was open when
  it was turned off; API and OAST listeners used a whole CPU when they ran
  out of file descriptors; the Organizer, OAST and plugin log views could
  show an older answer over a newer one.
- Faster: the app's startup script is 339 kB instead of 866 kB; sorting
  the history by length uses an index (schema v27).

### Added — Milestone 31b (Deferral sweep)
- ChosAI writes its answer as the model produces it, with a Stop button;
  "Record login" attaches the requests you make while logging in.
- ChosAgent: "Continue in a new task" for a task that stopped or was
  interrupted; nothing restarts by itself.
- Automation API: an OpenAPI 3.1 description (`/v1/openapi.json`) and
  engine events as Server-Sent Events (`/v1/events`);
  `chosproxy-cli events` and `chosproxy-cli openapi`.
- Plugins with `proxy.intercept` can also see responses and the requests
  of HTTP/2 clients.
- Crawler & tasks: choose a resource pool for a running task; a crawl
  graph for crawls and scans.
- Proxy → Listeners: a certificate per listener (ChosProxy's CA, a CA of
  its own, or a certificate file).
- Settings → Network: NTLMv2 platform authentication.
- Settings → Credentials: keep the keyring key in the macOS Keychain or
  behind a master password.
- ChosOAST: an SMTP listener that records mail (off by default; it never
  delivers or relays).
- Organizer: nested collections (`Parent/Child`) and drag and drop.
  Repeater: drag tabs between places and groups; a colour per tab.
- Site map: Conditions. Intercept: Format body, Compact and a GraphQL
  editor. Crawl, discovery and Fuzzer forms: saved settings with import
  and export.
- Scripts: `fn websocket(text, to_server)` in rewrite scripts, used by a
  "WebSocket script" Match & Replace rule.

### Added — Milestone 31 (Hardening, partly)
- Encrypted project files (SQLCipher): choose a password when creating a
  project, open it with the password, change it, and decide whether Save
  As, exports and backups are encrypted. Encrypted temporary projects use
  a key that exists only while they are open. See docs/PROJECT_FILES.md.
- Settings → Storage: what ChosProxy keeps in its temporary directory, and
  a cleanup that never touches the open project, other programs' files or
  anything outside that directory.
- Tools → Performance: memory, CPU, database size, storage rate, proxy
  connections, pools and tasks of this ChosProxy, read locally.

### Added — Milestone 30 (ChosAgent)
- ChosAgent: give a goal and an agent works on it with ChosProxy's own
  tools. Settings → ChosAgent sets Deny / Ask / Allow per tool; ChosProxy
  enforces them. Sending a request always waits for your approval of the
  exact request, needs a scope and only reaches targets in it.
- The agent keeps a plan, reads your messages between steps, and can be
  paused, resumed and stopped. Its requests show in HTTP history and the
  Logger (source `agent`).
- AI activity log (ChosAgent → AI activity log): every ChosAI question and
  every agent step, tool call and decision.
- ChosAI: follow-up questions, and "Review as session rule…" for the rule
  a login-flow answer suggests. See docs/AI.md.

### Added — Milestone 29 (ChosAI)
- ChosAI (Settings → ChosAI, off by default): explanations of requests,
  responses, JavaScript and findings, manual checks and fixes, a login
  flow helper, and report text, from a local model (Ollama, LM Studio,
  llama.cpp) or an external OpenAI-compatible provider you choose.
- Only the items you attach are sent, with credentials and secrets
  redacted; "Preview what is sent" shows the exact text, and external
  providers only receive a previewed context. See docs/AI.md.

### Added — Milestone 28 (REST API / Headless)
- Settings → Automation API: a token-protected JSON API on 127.0.0.1 for
  scripts (off by default; web pages cannot use it). See docs/API.md.
- `chosproxy-cli`: run ChosProxy without a window (`serve`) and drive it
  from the command line — scope, sends, crawls, scans, tasks, findings,
  reports, API definitions.

### Added — Milestone 27 (API / GraphQL workspace)
- Target → API workspace: import OpenAPI 3 / Swagger 2 (JSON or YAML),
  Postman collections and WSDL files; open any endpoint in Repeater or the
  Fuzzer; send the endpoints' example requests to the hosts you tick
  (in scope, read-only methods unless you ask, rate-limited).
- GraphQL: "Get GraphQL schema (introspection)" in the request menu; browse
  the schema, build queries and mutations with example variables, and see
  GraphQL operations in their own tab of the message viewer.
- Inspector: edit multipart field values and XML text and attributes.
- Repeater: "Format body" and "Compact" for JSON and XML bodies.
- Site map: each node lists the parameters seen and its latest requests.

### Fixed
- Resource pools: a request waiting for a back-off no longer blocks the
  pool status and other responses (M26 review); cancelled waits are no
  longer counted as waiting.

### Added — Milestone 26 (Resource pools & live tasks)
- Crawler & tasks → Resource pools…: shared limits (requests in flight,
  rate, random delay) for the crawler, Fuzzer, Sequencer, Repeater and
  plugins; a pool pauses when a server answers 429 or 503.
- Crawler & tasks → Live task…: a live passive audit or live passive crawl
  of the traffic as it arrives, with its own sources, filters and checks;
  it keeps going after the project is closed and reopened. Live tasks never
  send requests.
- New passive checks across requests: session token not renewed at login,
  token in a URL sent to another site, security headers missing on some
  pages. The background analysis also runs the JavaScript checks.
- Scans: a dashboard in the task panel, a note on every crawl request
  saying why it was made, and a check picker in the scan dialog.
- Plugins: the `proxy.intercept` permission lets a plugin change, block or
  answer in-scope proxy requests (see PLUGIN_SDK.md).

### Added — Milestone 25 (Advanced scanner engine)
- Crawler & tasks → New scan…: scan a site with a configuration from the
  library (crawl, then passive audit and JavaScript analysis of each
  distinct request once); the task log says why each item was audited or
  skipped. Save, import, export and schedule configurations. Scans send no
  attack payloads.
- JavaScript analysis: endpoints referenced by scripts, source maps,
  secret-looking values (masked) and DOM sinks.
- Findings → Suppressions and "Suppress similar…": rules that keep
  automatic checks from reporting false positives again.
- Fuzzer: "Auto §" now finds JSON, multipart and XML values too (▾ for
  path segments and headers); date payloads.
- Crawler: "Similar URLs" limits pages that differ only by ids.

### Added — Milestone 24 (Authenticated browser crawling)
- Settings → Credentials: an encrypted keyring for user names and secrets.
- Settings → Network → Platform authentication: Basic and Digest
  credentials answer 401 challenges for Repeater, the Fuzzer, the crawler
  and plugins.
- Project → Session rules: keep tool requests logged in — cookie jar,
  headers with extracted variables, expiry checks and login macros made
  from requests in the history.
- Crawler & tasks → Browser crawl…: crawl with the lab browser so links
  made by scripts are followed.

### Added — Milestone 23 (Extension platform / ChosHub)
- Plugin API v2 (v1 plugins keep working): plugins can send requests
  through ChosProxy (scope- and rate-limited, stored in the history), read
  the project on request, and add actions to request menus — each with its
  own permission. See PLUGIN_SDK.md.
- Signed plugins: `chosplugin` signs and verifies plugin folders; the
  Plugins view shows signatures, lets you trust a publisher, can refuse
  plugins not signed by a trusted publisher, and installs plugins from a
  folder.

### Added — Milestone 22b (Conditions & scripted tools)
- HTTP history and Logger → Conditions: filter on headers, bodies, status,
  length and more, with all/any; save the conditions as a filter script.
- Intercept rules: "Only URLs matching" a regular expression. HTTPS
  policy: never decrypt `host:port` matching a regular expression.
- Match & Replace rules for WebSocket messages (to server / to client).
- Scripts: import and export (imported scripts start switched off),
  payload scripts in the Fuzzer's processing, "Run script" in Repeater.

### Added — Milestone 22 (ChosScripts / ChosChecks)
- Tools → Scripts: write small sandboxed scripts (Rhai) and test them on
  stored requests. Check scripts add findings during passive analysis;
  filter and column scripts work in HTTP history and the Logger; action
  scripts appear in every request menu; rewrite scripts power new Match &
  Replace rules. See docs/SCRIPTS.md.

### Added — Milestone 21c (HTTP/2 through the proxy)
- Proxy → Listeners → "HTTP/2": browsers can use HTTP/2 for intercepted
  HTTPS. Requests and responses appear as `HTTP/2` in the history, with
  Match & Replace and Intercept; the proxy talks HTTP/2 to servers that
  offer it and HTTP/1.1 to the others.
- Invisible listeners: "Redirect to" sends all their traffic to one
  `host:port`.
- "Never decrypt these hosts" accepts a port (`example.com:8443`).

### Added — Milestone 21b (HTTP/2)
- HTTP/2: a request whose line says `HTTP/2` is sent over HTTP/2 (TLS with
  ALPN `h2`, or h2c) by Repeater, the Fuzzer, the Sequencer and the crawler.
  The history shows `HTTP/2 200` responses and notes what had to change.
- Repeater → Options → Protocol: switch a tab between HTTP/1.1 and HTTP/2.
- Single-packet attack: a parallel group of HTTP/2 tabs to one server sends
  the final frames of every request together, in one packet.
- Proxy → HTTPS → Import CA…: use your own CA (certificate and unencrypted
  PKCS#8 key); the previous one is kept.

### Added — Milestone 21 (Advanced proxy & network)
- Settings → Network: upstream HTTP and SOCKS5 proxies chosen per
  destination (with credentials and a Direct exception), host name
  overrides, and connect/response/idle timeouts. The proxy, Repeater and
  every tool follow them. See docs/NETWORK.md.
- Invisible listeners for clients that do not know they use a proxy: plain
  HTTP by its Host header, TLS by its server name, on a configurable port.
  Requests that would loop back into the proxy are refused (508).
- Intercept → Match & Replace: automatic changes to the request line,
  headers and body, the status line, response headers and body; literal or
  regex; compressed bodies decoded first; the history keeps the original.
- Repeater remembers each tab's "Follow redirects" setting. Project schema
  v20.

### Added — Milestone 20b (Editor & workflow conveniences)
- Command palette: type three or more characters to find stored requests
  and open them.
- Scope → "Import a list of hosts or URLs…": hosts, `*.domain` entries
  and URLs become include or exclude rules.
- Repeater: "Follow redirects" option per tab (never, on the same site, in
  scope, always); a note per tab. Project schema v19.
- Message editors: right-click or Shift+F10 menu with copy, decode, search
  the project, and copy as curl, Python or fetch.
- Inspector: edit JSON body field values in Intercept and Repeater.
- HTTP history: saved filters.
- Settings → Keyboard shortcuts: change or clear any global or view
  shortcut.

### Added — Milestone 20 (Engagement tools)
- Content discovery (Tasks → Discover content, or "Discover content
  here…" on requests and site map nodes): finds unlinked folders and files
  with built-in, site-map and custom names, recognises "not found" pages
  that answer 200, searches found folders, stays in scope and stores only
  the hits.
- "Generate CSRF PoC…" and "Test clickjacking (FrameLab)…" on every
  request: reviewable HTML proofs of concept with their limits explained.
- Target analyzer (parameters, HTML comments, scripts) and "Find
  references to this URL".
- Site map snapshots and comparison.
- Scheduler: run a crawl or a discovery later, once or repeatedly, while
  the project is open. Project schema v18.

### Added — Milestone 19 (DOM Inspector)
- DOM Inspector (Tools → DOM Inspector): opens the lab browser (Chromium,
  Chrome, Edge, Brave) with instrumentation that shows where values from
  the URL, the referrer, window.name, cookies or postMessage reach
  dangerous DOM sinks, as Source → Transformation → Sink with the code
  location and stack; also client-side redirects, postMessage listeners
  without an origin check, prototype pollution and DOM clobbering, with
  findings linked to the page response.
- A session canary with "+ canary in #/?" and "Open in the lab browser".
- Lab server: intentionally vulnerable pages under /dom/. Project schema v15.

### Changed — Milestone 19
- The sidebar fits its 25 entries on shorter windows.

### Added — Milestone 18 (OAST / ChosOAST)
- OAST (Tools → OAST): unique payload addresses to put in requests; when a
  target looks one up or fetches it, the interaction (DNS, HTTP or HTTPS,
  with time, origin and raw data) appears live, linked to the stored
  request that carried it, and an "External service interaction" finding
  is created.
- Local mode (a server inside ChosProxy, loopback by default) and
  Self-hosted mode with the new `chosoast` server for a domain you control
  (docs/OAST.md); off by default.
- "New OAST payload" in the command palette; OAST filter in Findings.
  Project schema v14.

## [1.0.0] — 2026-11-11

First stable release of the **Core** tier: Proxy, Intercept, HTTP history,
Repeater, Site map, WebSockets, Decoder, Comparer and Organizer, plus the
Pro tools built so far (crawler, passive scanner, Fuzzer, Sequencer,
plugins, reports). Packages: `.deb` and AppImage for Kali Linux x86_64.
What each Core capability does and does not cover yet is recorded in
docs/audits/2026-11-11-m17-core-audit.md.

### Added — Milestone 17 (1.0)
- HTTP history: sort by any main column (click its header) and filter by
  MIME type.
- "Copy as curl command" in every request menu.
- Inspector: JSON and multipart body fields, and the cookies a response
  sets with their attributes.
- Global search also searches WebSocket messages.
- Hex view: select bytes (click, then Shift+click) and send them to the
  Decoder or copy them as hex.
- Site map: right-click / Shift+F10 menu on hosts, folders and endpoints.

### Security — Milestone 17
- Bundled SQLite updated for RUSTSEC-2026-0459 (CVE-2026-119375); building
  from source needs Rust 1.89.

### Added — Milestone 16 (Performance & hardening)
- Keyboard shortcuts: Ctrl/Cmd+1…9 open the main views, Alt+↑/↓ step
  through the sidebar, Ctrl/Cmd+Shift+I turns Intercept on or off, and `?`
  (or "Keyboard shortcuts" in the palette) lists them all.
- "Getting started" on the dashboard now includes defining the scope.
- Robustness tests that feed corrupted input to every parser.

### Changed — Milestone 16
- The Fuzzer and Sequencer refuse a target outside the scope when a scope
  is defined (as the crawler already did).
- The sidebar fits windows down to about 760 px high without scrolling.
- Fuzzer results re-render less while an attack runs.

### Added — Milestone 14 (Plugins)
- Plugins: programs in any language that receive the project's traffic,
  add findings and annotate the history, each only with the permissions you
  grant (`network.read`, `scanner.passive`, `project.write`). They run as
  separate processes while a project is open; every message is checked,
  refusals are logged and recorded in the audit log, and a plugin whose
  files change must be approved again.
- Plugins view: install the HelloPlugin example, review and grant
  permissions, enable or disable, see status and log. SDK guide in
  PLUGIN_SDK.md; lab page `/debug-token` for the example.

### Added — Milestone 12 (Sequencer)
- Sequencer (Tools → Sequencer): repeat a request that issues a token and
  collect one token per response from a cookie, a header or a regular
  expression ("Send to Sequencer" on any request suggests where the token
  is), or paste tokens you already have.
- Analysis: repeats, lengths, character set, estimated entropy per
  position, bit balance, monobit and character-frequency tests, fixed parts
  and sequential tokens — each as an explained observation, with charts and
  a reminder that no single number proves a token is secure.
- Lab server: `/token` (random) and `/token/sequential` (counter). Captures
  run as tasks; their traffic is stored as "sequencer". Project schema v13.

### Added — Milestone 11 (Fuzzer)
- Fuzzer (Tools → Fuzzer): mark payload positions with § (by hand or "Auto
  §" for query, cookie and form values), choose an attack mode (sniper,
  battering ram, pitchfork, cluster bomb) and payloads — your own list or
  file, numbers, character combinations or built-in test lists (no
  credential lists) — with processing rules (prefix, suffix, replace, case,
  URL/Base64/hex/HTML encoding, hashes).
- Attacks run in the background as tasks with pause, resume and stop,
  concurrency, requests per second, timeout and retries, and a baseline
  request with the original values. Results show payload, status, length,
  time, errors, grep matches and extracted values; sort them, keep only
  matches or statuses that differ from the baseline, and inspect each
  request and response.
- "Send to Fuzzer" in the request menu and "Fuzz" in Repeater. Fuzzer
  traffic is stored as "fuzzer" and checked by passive analysis. Project
  schema v12.

### Added — Milestone 10 (Crawler and tasks)
- Crawler: follows links, frames, scripts, GET forms, redirects, URLs in
  JavaScript, robots.txt and sitemaps to map a site. It only sends GET
  requests, never submits POST forms (they are listed in its log), skips
  links that look like logging out or deleting, stays in scope (or on the
  start sites while no scope is defined) and can crawl as the logged-in
  user with the cookie jar. Limits: depth, requests, concurrency, requests
  per second, timeout, query variants per path.
- Tasks: crawls run in the background with pause, resume and stop,
  progress, errors, duration and a log, saved in the project ("Crawler &
  tasks" view). "Crawl from this URL" in the request menu and "New crawl"
  in the command palette.
- Crawler traffic is stored as "crawler" (Logger, site map, search) and is
  checked by passive analysis. Project schema v11.

### Added — Milestone 9 (Passive scanner, completed)
- Passive analysis: traffic going through the proxy (and Repeater) is
  checked in the background for 18 common weaknesses — cookie flags, HSTS,
  nosniff, clickjacking, CSP, software versions, permissive CORS, mixed
  content, passwords over HTTP, secrets in URLs, JWT problems, error and
  database messages, private IP addresses, directory listings, cacheable
  authenticated responses and exposed files. Nothing is sent.
- Each problem becomes one finding (source "passive") with an explanation,
  how to fix it and up to five requests as evidence. False positives and
  accepted risks stay quiet; fixed findings reopen if the problem returns.
- Findings view: source filter, analysis status and "Analyze all again".
  Settings: turn analysis on or off, in-scope only, Repeater traffic and
  each check. Project schema v10.

### Added — Milestone 4 (Repeater, completed)
- Tab groups: name and colour, tabs kept together, added and removed from
  the tab menu (right-click or Shift+F10). "Send group" sends every tab in
  sequence, in sequence over a single connection, or in parallel with the
  last byte of every request written at the same moment (race conditions).
- Cookie jar per project, filled from the Set-Cookie headers of proxy and
  Repeater responses; tabs with "Use the cookie jar" add the matching
  cookies when sending (the editor text is not changed). View, edit, delete
  and clear it from Repeater.
- "Reuse the connection" per tab keeps the connection open between sends
  (keep-alive); reused connections and jar cookies are shown with the
  response. Tab options moved to an "Options" menu.
- Shared right-click menu in HTTP history, the Logger, Search and the
  Organizer: Repeater, Organizer, finding, Comparer, Decoder, scope, copy
  URL. Repeater tabs show the status of their last send. Project schema v9.

### Added — Milestone 4 (Repeater, first part)
- Repeater (Tools → Repeater): edit a request and send it again and again.
  Tabs are kept in the project and can be renamed, duplicated, reordered
  and closed; each has a target, a raw editor with the Inspector, Send
  (Ctrl/Cmd+Enter), Cancel and an "Update Content-Length" switch.
- The response shows status, size and timing (DNS, connect, TLS, server,
  download). Every send is kept in the tab's history (go back and forward,
  restore an earlier request) and in the Logger as Repeater traffic;
  "Diff previous" compares two responses and "Follow redirect" prepares and
  sends the next request of a redirect.
- "Repeater" action in HTTP history, the Logger, Search, the Organizer and
  Intercept (a paused request, with your edits). Project schema v8.

### Added — Milestone 15 (Reporting)
- Report view: build an assessment report from the findings you choose,
  with executive summary, target and scope, methodology, findings,
  remediation, technical details and evidence; live preview.
- Export as a self-contained HTML page (no scripts, nothing loaded from the
  network), Markdown or JSON. Credentials in evidence headers are redacted
  by default and evidence can be shortened or left out. Exports are recorded
  in the project audit log.

### Added — Milestone 9 (in progress)
- Findings: document a problem from any request ("Finding" in HTTP
  history, the Logger and Search) or add more requests as evidence; review
  statuses (false positives and accepted risks are kept); severity counts on
  the dashboard. Project schema v7.

### Changed — performance
- Global search is about 4× faster on large projects.
- Live traffic refreshes only the list on screen and counters less often;
  the history uses bounded memory and no longer flickers while reloading.
- Views load on demand (much smaller startup script).

### Fixed
- The WebSockets view and the event log stopped refreshing during continuous
  traffic.
- Importing settings no longer repeats configuration warnings.

### Added — Milestone 8 (Projects)
- Project view: Save As (a temporary project becomes a file), export a copy
  (optionally in-scope traffic only), Markdown notes, audit log.
- Automatic, verified project backups with rotation and restore (as a copy).
- Scope per project ("Use global scope" / "Override for this project").
- Settings import/export (JSON) with validation and a diff preview; listener
  credentials are never exported.
- Organizer collections. Project schema v6.

### Added — Milestone 7 (WebSockets)
- WebSocket capture: connections linked to their handshake and every
  message (time, direction, type, length, content) in Proxy → WebSockets,
  with direction and control-frame filters, payload search and JSON export.
- Intercept WebSocket messages per direction: edit, forward or drop.
- Handshake extensions such as compression are removed so messages stay
  readable (noted on the handshake request). Project schema v5.
- Lab server WebSocket echo (`/ws`) and demo page (`/ws-demo`).

### Added — Milestone 6 (Utilities)
- Decoder: URL, Base64, Base64 URL, hex, HTML entities, Unicode escapes,
  gzip/zlib, pretty JSON, JWT inspection and MD5/SHA hashes as editable
  chains; smart decode; "Decode selection" in every message viewer.
- Comparer: line, word and byte diffs and a structured HTTP comparison;
  "Compare" on requests and responses.
- Organizer: keep requests with status, severity, tags and notes (project
  schema v4).
- Render view for HTML responses (sandboxed, no scripts or network).

### Added — Milestone 5 (Target)
- Scope: include/exclude rules (protocol, host with optional subdomains or
  regex, port, path prefix or regex) in `scope.toml`, a URL tester, "Add to
  scope"/"Exclude from scope" actions and a scope indicator in the top bar.
- Site map built automatically from captured traffic (origin → folders →
  endpoints) with methods, statuses, MIME types, sources, first/last seen
  and node marks (interesting, reviewed, ignore, favorite).
- Global search over URLs, request/response headers, decoded bodies and
  notes (text or regex, match case, in scope only).
- HTTP history: in-scope column, regex URL filter, in-scope / annotated /
  source filters, site map node filter, row highlights and notes.
- Intercept rule "only in-scope targets". Project schema v3 (upgraded
  automatically).
- Inspector tab in request viewers (query and form parameters, cookies,
  headers) and an editable Inspector beside the Intercept editor.
- Logger view with every tool's traffic and a follow mode; HTTP history now
  lists proxy traffic only.
- Optional proxy authentication per listener (HTTP Basic; only a salted
  PBKDF2 hash is stored); rejected logins are counted and reported.
- Event log view of the session (listener, project, scope, CA events,
  warnings and errors), linked from the notifications panel.

### Changed
- The proxy's upstream connector moved into the shared request crate
  (`chosproxy-request`, ADR-0009) as the first step of Milestone 4.

### Added — Milestone 3 (Intercept)
- Intercept requests and responses: pause, edit as text (byte-exact for
  unedited messages), forward, forward & intercept response, drop, forward
  all, unpack compressed/chunked bodies, automatic Content-Length update.
- Intercept rules (hosts, methods, parameters, static assets, response
  types), top-bar switch with waiting count, command palette actions.
- History keeps the original of edited messages and marks edited/dropped
  exchanges (project schema v2, upgraded automatically).

### Added — Milestone 2 (HTTPS)
- Local certificate authority generated per installation; HTTPS
  interception through CONNECT with per-host certificates; upstream TLS
  verification with explicit per-host exceptions; passthrough hosts and an
  interception switch.
- HTTPS & lab browser page: CA status and explanation, export (PEM/DER),
  regeneration, interception policy, manual trust instructions.
- Isolated lab browser (Chromium family or Firefox) launched from the top
  bar, dashboard or command palette; temporary profile deleted on exit.
- CA certificate download at `http://chosproxy/cert`; counters and notices
  for clients that reject the interception certificate.
- HTTPS listener in the lab server.

### Fixed
- Saving proxy listeners no longer drops other proxy settings.

### Added — Parity planning and audit (2026-10-22)
- Specification and parity addendum stored in `docs/`; capability parity
  matrix (134 rows), milestones M18–M32, core seams (ADR-0009, ADR-0010).
- Kali x86_64 packaging in a Kali container (`scripts/build-kali.sh`),
  `.deb` smoke test, clean backup script; `cargo audit`/`pnpm audit` in CI.

### Fixed
- History keyboard navigation keeps the selected request when new traffic
  arrives; history reloads when another project is opened; clearing filters
  no longer re-applies the previous search.

### Added — Milestone 1 (HTTP proxy)
- Proxy listeners: multiple, start/stop/restart, interface and port
  selection, loopback-only unless remote access is explicitly allowed.
- Wire-exact HTTP/1.1 relay: absolute-form requests, keep-alive and upstream
  connection reuse (with stale-connection retry for bodyless requests),
  chunked bodies with trailers, `Expect: 100-continue`, interim 1xx,
  HEAD/204/304, close-delimited responses, protocol upgrades (relayed, not
  captured), `CONNECT` tunnels (not decrypted), loop protection and a local
  info page, 400/431/502/504 proxy errors.
- Capture to the project database with per-message size cap and truncation
  flags; Content-Length/Transfer-Encoding conflicts flagged.
- HTTP history UI: virtualized table, URL/method/status/static filters,
  sort order, keyboard navigation; request/response viewer with Pretty
  (de-chunked, decompressed, JSON formatted), Raw and Hex modes.
- Local lab server (`chosproxy-lab`) and end-to-end tests.

### Added — Milestone 0 (Foundation)
- Rust workspace (`core`, `http`, `storage`, `proxy`, `app`, `lab`) and Tauri
  2 + React 19 + TypeScript desktop app.
- Configuration split into per-section TOML files with atomic, private
  writes; unusable files are preserved, never silently overwritten.
- Project databases: SQLite with versioned migrations, batched writer thread,
  compressed body blobs; temporary and persistent projects.
- Logging with runtime level changes and secret redaction helpers.
- Welcome flow, dashboard, settings, command palette, themes, status bar.
- Generated TypeScript bindings, CI workflow, documentation and ADRs.
