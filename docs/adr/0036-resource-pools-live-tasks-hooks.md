# ADR-0036: Resource pools, live tasks, cross-request checks and plugin hooks

* Status: Accepted
* Date: 2026-11-12

## Context
M26 covers these rows of the parity matrix:
* resource pools shared across tools (INTRUDER-006, REPEATER-001,
  PLATFORM-001);
* live tasks (SCANNER-009);
* the M25 deferrals: JavaScript analysis in the background analyzer and
  multi-request analysis (SCANNER-001/003), a scan dashboard and
  per-request crawl reasons (SCANNER-006), per-check selection in the scan
  UI (SCANNER-007);
* extensions that block or answer live traffic (EXT-002, deferred by
  ADR-0033 until live tasks and pools existed).

M26 delivered passive live tasks and pools (ADR-0035). Active audit is
separate M13 work, selected next on 2026-11-14 and not yet implemented.

## Decisions
1. **Resource pools** (`engine/pool_ops.rs`, `pools.toml`).
   * A pool is a name, requests in flight, requests per second, a random
     delay (0–60 s) and an optional back-off: a 429 or 503 response pauses
     the pool for its `Retry-After` in seconds (5 s without one, at most
     60 s).
   * Pools are assigned per tool: crawler (scans and content discovery
     included), Fuzzer, Sequencer, Repeater, plugins. A tool without a
     pool keeps only its own limits, so nothing changes until a pool is
     assigned.
   * The pool slot is taken in `session_send`, the one path every tool
     send goes through (and in the Repeater's kept connections). Each
     task's own limiter still applies on top.
   * Parallel Repeater groups (last-byte synchronisation) and the browser
     crawl's page loads are not pooled; pooling them would break what they
     are for, or they do not go through the Request Engine.
   * Waits of a second or more and back-offs are written in the history
     notes; counters (in flight, waiting, sent, backed off) are live in
     the Resource pools dialog.
2. **Cross-request checks** (`chosproxy_scanner::cross`).
   * A `CrossAnalyzer` is fed the traffic in id order and keeps bounded
     state per origin (1000 origins, 2000 token pages).
   * Checks: a session cookie sent to a successful login, not renewed by
     it, and accepted afterwards; a URL with a token parameter sent as
     `Referer` to another host; security headers (HSTS, CSP,
     anti-framing) present on some HTML pages of an origin and missing on
     others.
   * Detections can name an earlier exchange (the page missing a header,
     seen before the page that has it), so the analyzer returns
     `(exchange id, issue)` pairs.
   * Used by the background analyzer (state reset when a project opens or
     is re-analyzed), by scans (every item, duplicates included) and by
     live audits. The background analyzer now runs the JavaScript checks
     too; all checks are listed and can be turned off in the settings.
3. **Live tasks** (`engine/live_ops.rs`, task kind `live`).
   * Live passive audit: the passive, JavaScript, cross-request and script
     checks on matching traffic, with its own check selection.
   * Live passive crawl: links, scripts, forms and redirects found in
     matching responses that nobody requested, stored as task results
     (`discovery_results`, status 0) and marked with the status once they
     are requested.
   * Filters: traffic sources, in scope only, URL text; new traffic only or
     from the start.
   * A live task never sends a request.
   * It keeps its cursor in its configuration. When the project closes it
     is suspended (`Control::Suspend`), not cancelled, and it resumes,
     running or paused, when the project is opened again. Other task kinds
     are still recorded as cancelled.
4. **Scan dashboard and crawl reasons.**
   * Crawl exchanges are stored before their links are queued, so each
     queued URL knows the exchange it was found on (`Entry.via`). Every
     crawl request carries a note: "Crawler: start URL.", "a link on #12
     (depth 2)", "redirected from #7"…
   * A scan saves a `ScanSummary` (phases with their state, crawl and audit
     counters, scripts analyzed, findings created and updated, distinct
     issues by severity) in a new `tasks.summary` column (schema v22). The
     task panel shows it as a dashboard.
   * The scan and live-audit dialogs have a check picker grouped by single
     message, JavaScript and across requests.
5. **Plugin request hooks** (`proxy.intercept`, API v2).
   * The proxy has a `RequestHook` trait. ChosProxy's implementation asks
     plugins that registered a hook, in plugin id order, after Match &
     Replace and before Intercept, on HTTP/1.1 client connections.
   * Plugins answer `continue`, `replace`, `block` (403 with the reason) or
     `respond` (the response, then the connection is closed).
   * Hooks apply **only while a scope is defined and only to in-scope
     requests**, never to WebSocket handshakes, binary requests or requests
     over 1 MiB.
   * A plugin has 2 s to answer. Timeouts and unusable answers let the
     request go on unchanged (fail open).
   * Every change, block and answer is noted in the history entry; blocks
     and answers also go to the plugin's log, and registering a hook is
     audited. The permission is granted explicitly like every other.

## Consequences
* Several automated tools can share one politeness budget for a fragile
  target, and they slow down when it asks them to.
* Proxy browsing can be audited and mapped continuously, with filters and
  check selections of its own, across project sessions.
* Session fixation, Referer token leaks and partial header coverage are
  reported from ordinary browsing.
* Scans explain themselves at a glance (dashboard) and per request (notes).
* Plugins can implement traffic policies (stubs, blocks, rewrites) within
  the scope, with a visible trail.

## Deferred
* Hooks for HTTP/2 client connections and for responses, and a per-task
  (rather than per-tool) pool choice → M31.
* Data-flow analysis of scripts → M31.
* Live active audit → M13 (selected next; implementation pending).
