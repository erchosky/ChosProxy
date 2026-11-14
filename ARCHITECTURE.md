# ChosProxy Architecture

This document describes how ChosProxy is put together, why, and where each
responsibility lives. Decisions that are expensive to reverse are recorded as
ADRs in [`docs/adr/`](docs/adr/).

> ChosProxy is a tool for **authorized** security testing only: your own
> systems, local labs (Juice Shop, DVWA, WebGoat, Web Security Academy),
> CTFs and targets you have explicit written permission to test.

## 1. Goals that shape the architecture

In priority order (from the product spec):

1. **Security** of the tool itself (local CA key, project files, untrusted
   traffic rendered in a privileged desktop app).
2. **Stability** — the proxy must never take down the browser session because
   the UI is slow or a message is malformed.
3. **Traffic fidelity** — what the user sees is byte-for-byte what crossed the
   wire, and what the user sends is byte-for-byte what they typed.
4. Maintainability, performance, UX, extensibility, compatibility, looks.

## 2. High-level view

```
┌───────────────────────────── Desktop process ─────────────────────────────┐
│                                                                           │
│  WebView (React + TypeScript)      ◄── typed IPC (Tauri commands/events)  │
│   features/ · components/ · services/ipc · state/                        │
│                                         │                                 │
│  ───────────────────────────────────────┼──────────────────────────────── │
│  chosproxy-desktop (Tauri glue, thin)   ▼                                 │
│                                                                           │
│  chosproxy-app  ── Engine: owns project, config, proxy, event bus        │
│      │                │                    │                              │
│      ▼                ▼                    ▼                              │
│  chosproxy-proxy   chosproxy-storage   chosproxy-core                     │
│  (listeners,       (SQLite, migrations, (config, brand, ids, errors,      │
│   HTTP/1.1 relay,   history repo,        events, redaction)               │
│   upstream client)  blob compression)                                     │
│      │                                                                    │
│      ▼                                                                    │
│  chosproxy-http  (wire-exact HTTP/1.1 codec, message model, MIME,        │
│                   content decoding)                                       │
└───────────────────────────────────────────────────────────────────────────┘
```

**The networking engine never depends on the UI.** `chosproxy-app` exposes a
plain Rust API (`Engine`) plus a broadcast event stream. The Tauri crate only
translates between that API and IPC. The same `Engine` will back the future
CLI / headless mode (spec §59) without changes.

## 3. Workspace layout

```
chosproxy/
├── branding.json            single source of truth for the product name
├── Cargo.toml               Rust workspace
├── package.json             pnpm workspace root (frontend tooling)
├── apps/
│   └── desktop/             Tauri app: React UI (src/) + Rust glue (src-tauri/)
├── crates/
│   ├── core/                config, branding, ids, errors, events, redaction
│   ├── http/                HTTP/1.1 wire codec + message model
│   ├── tls/                 local CA, per-host certificates, upstream TLS
│   ├── storage/             SQLite project database, migrations, repositories
│   ├── request/             shared Request Engine: upstream TCP/TLS connections
│   ├── decoder/             Decoder: encodings, compression, hashes, smart decode
│   ├── websocket/           WebSocket frame codec and message assembly
│   ├── proxy/               listeners, client/upstream relay, capture
│   ├── app/                 Engine: composes everything, UI-agnostic
│   └── lab/                 local deliberately-simple test server (tests only)
├── docs/adr/                architecture decision records
├── fixtures/                static test inputs (raw HTTP messages, …)
├── scripts/                 developer scripts
└── .github/workflows/       CI
```

### Deviations from the proposed structure (and why)

| Proposed | Chosen | Reason |
|---|---|---|
| One crate per future tool (`repeater/`, `crawler/`, `scanner/` …) created up-front | Crates are created **when their milestone starts** | Empty crates are "fake features" in code form; they rot and invite `TODO`s. The boundaries are documented here so new crates slot in predictably. |
| `crates/tls`, `crates/websocket` | Created in M2 / M7 | Same reason. |
| `packages/ui` shared UI package | UI lives in `apps/desktop/src` | There is exactly one frontend consumer. Splitting now adds build plumbing with no reuse benefit. Components are kept free of Tauri imports (all IPC goes through `services/`) so extraction later is mechanical. |
| — | `crates/app` | A UI-agnostic composition root (`Engine`) so Tauri, a CLI and tests all drive the same code. |
| — | `crates/lab` | Spec §46 lab server, as a library (spawned in-process by integration tests) and a binary (`chosproxy-lab`) for manual testing. |

Planned crates and the milestone that introduces them: `tls` (M2),
`intercept` logic lives in `proxy` (M3), `request` (M4), `repeater` (M4);
scope lives in `core::scope` and the site map is a storage query (M5, ADR-0013); `decoder`/`comparer` (M6), `websocket` (M7), `scanner` (M9),
`crawler` (M10), `fuzzer` (M11), `sequencer` (M12), `audit` (M13),
`plugins` (M14), `reporting` (M15).

## 4. Crate responsibilities

### `chosproxy-core`
* `brand` — product name/identifiers loaded from `branding.json` at compile
  time (spec: the name must be changeable from one place).
* `config` — typed settings sections, each its own TOML file in the user
  config dir (`settings.toml`, `proxy.toml`; later `scope.toml`, …). Atomic
  writes, `0600` permissions, never overwrite a file we failed to parse.
* `paths` — XDG-style locations via `directories`.
* `event` — `EngineEvent` enum broadcast to UIs.
* `redact` — header/value redaction used anywhere traffic may be logged.
* `exchange` — `CapturedExchange`, the unit every traffic producer hands to
  storage (re-exported by storage as `NewExchange`).
* `source` — `Source` enum (`Proxy`, `Repeater`, `Crawler`, …) attached to
  every stored message so automated traffic is always attributable (spec §21).

### `chosproxy-http`
* Wire-exact HTTP/1.1 codec (ADR-0003): request/response **head** parsing via
  `httparse`, body framing per RFC 9112 (Content-Length, chunked incl.
  trailers, close-delimited, no-body statuses), streaming body reader that
  yields *raw framing bytes* so forwarding is lossless.
* Message model: raw bytes are the source of truth; parsed views (method,
  target, headers, query) are derived and never re-serialized implicitly.
* Content helpers: MIME classification, de-chunking, gzip/deflate/br decoding
  for *display only*.

### `chosproxy-tls`
* `CaStore`/`CertificateAuthority` — per-install CA (ECDSA P-256), files in
  `<data>/ca/` (`0700`/`0600`), validated on load, never overwritten
  implicitly.
* `Interceptor` — per-host leaf certificates (SNI), shared in-memory leaf
  key (enables the Chromium SPKI pin), cached; server TLS configs.
* `UpstreamTls` — client configs: verified (system + Mozilla roots) and
  "skip chain verification" for explicit per-host exceptions.

### `chosproxy-storage`
* One SQLite file per project (ADR-0004), WAL mode, versioned migrations.
* Dedicated writer thread with batched transactions; a small pool of
  read-only connections for queries. Callers never block the async runtime:
  async wrappers use `spawn_blocking`.
* Message bodies live in a `blobs` table, zstd-compressed above a threshold,
  loaded lazily by id. History listing never touches blob data.

### `chosproxy-proxy`
* `ListenerManager` — start/stop/restart independent listeners
  (`127.0.0.1:8080` by default), loopback-only unless remote access is
  explicitly enabled per listener.
* Connection handler — reads client requests, resolves the upstream (absolute
  form, `CONNECT`, or origin form + `Host`), relays to upstream with the
  `chosproxy-http` codec, streams the response back, records the exchange.
* Bounded capture: bodies are forwarded as they stream; the recorder keeps up
  to a configurable cap and flags truncation, so a 2 GB download cannot
  exhaust memory.
* Loop protection (requests addressed to our own listener are refused).
* `CONNECT` (M2, ADR-0011): after answering 200 the first client bytes
  decide — TLS is terminated with a per-host certificate from
  `chosproxy-tls` and the decrypted HTTP/1.1 runs through the same relay
  (connection switches to "tunnel mode" with a fixed origin); plain HTTP is
  captured; other protocols, passthrough hosts and disabled interception are
  opaque tunnels. Streams are boxed (`BoxIo`) so TCP and TLS share code.
* The proxy serves `http://chosproxy/` (info page) and `/cert` (public CA
  certificate).
* Intercept (M3, ADR-0012): `InterceptQueue` holds the switch, rules and
  paused messages. The relay reads a matching message completely, parks it
  in the queue and waits for a `Decision` (forward bytes / drop) before
  connecting upstream (requests) or delivering (responses). The queue is
  bounded in count and bytes; the engine converts the user's text edits back
  to bytes with `chosproxy_http::edit`.

### `chosproxy-app`
* `Engine` — opens a project (temporary or persistent), loads config, owns the
  `ListenerManager`, the storage handle and the event bus. All UI operations
  are methods on `Engine`.
* `CaManager` — loads/creates the CA at startup, exports, regenerates and
  hands the `TlsContext` to the proxy.
* `browser` / `BrowserSessions` — detects installed browsers and runs one
  isolated lab browser (temporary profile, deleted on exit).

### `apps/desktop`
* `src-tauri` — Tauri commands are one-line delegations to `Engine`; events
  are coalesced (≤ 5 Hz) before being emitted to the WebView.
* `src` — React + TypeScript (strict). Structure:
  `features/<area>/` (screens), `components/` (reusable UI, incl. the HTTP
  message viewer), `services/` (the *only* place that calls Tauri), `state/`
  (Zustand stores), `bindings/` (TS types generated from Rust via `ts-rs`).

## 5. Key data flows

### Proxy capture (M1)

```
browser ──TCP──► listener ──► ClientConn::next_request()
                                │ head parsed (httparse), body streamed
                                ▼
                         Upstream::connect(host,port)  ◄── per-connection reuse
                                │ forward raw head + raw body bytes
                                ▼
                         response head parsed, body streamed to browser
                                │ tee (≤ cap) into Recorder
                                ▼
                    storage writer thread (batched txn) ──► SQLite
                                │
                         EngineEvent::HistoryAppended ──► coalesced ──► UI
```

### UI history view
The UI asks for `count(filter)` and pages of lightweight rows
(`HistoryRow`, no bodies) for the visible window of a virtualized table.
Selecting a row fetches the full exchange (raw bytes, base64 over IPC,
capped for display with on-demand full load).

## 6. Interfaces (stable contracts)

```rust
// chosproxy-app
impl Engine {
    pub async fn open(opts: OpenProject) -> Result<Engine, AppError>;
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent>;
    pub async fn start_listener(&self, id: ListenerId) -> Result<(), AppError>;
    pub async fn stop_listener(&self, id: ListenerId) -> Result<(), AppError>;
    pub async fn history_page(&self, q: HistoryQuery) -> Result<HistoryPage, AppError>;
    pub async fn exchange(&self, id: ExchangeId) -> Result<ExchangeDetail, AppError>;
    pub async fn update_proxy_config(&self, cfg: ProxyConfig) -> Result<(), AppError>;
}

// chosproxy-proxy (CapturedExchange is defined in chosproxy-core)
pub trait CaptureSink: Send + Sync + 'static {
    fn record(&self, exchange: CapturedExchange); // must not block
}

// chosproxy-storage
impl ProjectDb {
    pub fn open(path: &Path) -> Result<ProjectDb, StorageError>;
    pub fn submit(&self, ex: NewExchange) -> Result<(), StorageError>;   // non-blocking, may be QueueFull
    pub fn insert(&self, ex: NewExchange) -> Result<ExchangeId, StorageError>; // waits for the id
    pub fn history_page(&self, q: &HistoryQuery) -> Result<HistoryPage, StorageError>;
    pub fn exchange(&self, id: ExchangeId) -> Result<Option<StoredExchange>, StorageError>;
}
```

`CaptureSink` is the seam where Intercept (M3), the passive scanner (M9),
site map (M5) and plugins (M14) attach without the proxy knowing about them.

## 7. Concurrency model

* Tokio multi-thread runtime; one task per client connection.
* Bounded channels everywhere between producers and storage/UI
  (back-pressure instead of unbounded memory growth). If the storage queue is
  full the proxy **keeps forwarding** and the capture is dropped with a
  counted, surfaced warning — availability of the user's browsing beats
  completeness of the log.
* SQLite access is synchronous on dedicated threads.

## 8. Error handling

* Library crates use `thiserror` enums; `anyhow` only in binaries/tests.
* No `unwrap()`/`expect()` on runtime data. `expect()` is allowed only for
  invariants that are bugs if violated, with a message stating the invariant.
* Errors crossing IPC are serialized as `{ kind, message }`; the UI shows them
  in context (inline/toast/status bar) — never silently dropped.

## 9. Security architecture (summary — see THREAT_MODEL.md)

* WebView never renders response content as HTML in the privileged context.
  Message bodies are shown as text in CodeMirror; the future "Render" view
  will use a sandboxed iframe without script or same-origin rights and with
  network access blocked by CSP.
* Strict CSP, Tauri capabilities limited to what commands need, no shell
  plugin, no remote URLs in the main WebView.
* Listeners bind to loopback by default; enabling remote access is explicit
  and warned about.
* Logs redact credentials and cookies by default.
* No telemetry, no network calls except traffic the user directs.

## 10. Technical risks

| Risk | Impact | Mitigation |
|---|---|---|
| HTTP edge cases (chunked trailers, 1xx, `Expect: 100-continue`, close-delimited bodies, pipelining, HEAD) | Broken sites through proxy, wrong history | Own codec with exhaustive unit tests + fixtures; lab server routes for each case; fuzzing in M16 |
| Request smuggling-style ambiguity (CL + TE) | Proxy desync | Treat `Transfer-Encoding` as authoritative, flag the conflict in history notes, never "fix" bytes silently |
| Huge bodies / streaming downloads | Memory exhaustion | Streaming forward + capped capture with truncation flag |
| SQLite write contention under load | UI stalls, dropped captures | Single writer thread, batched transactions, WAL, bounded queue with visible drop counter |
| IPC volume (tens of thousands of rows) | Laggy UI | Virtualized table, paged queries, coalesced events, no bodies in list payloads |
| Rendering hostile content in WebView | Code execution in privileged context | Text-only views; sandboxed iframe later; CSP; never `dangerouslySetInnerHTML` with traffic |
| CA private key theft (M2) | MITM of the user | Per-install key, `0600` file, never exported, regeneration flow |
| HTTP/2 / HTTP/3 (later) | Large codec surface | Version-agnostic message model; ALPN negotiated per upstream in M2+ |
| WebKitGTK differences on Kali | UI glitches | CI builds on Debian-family Linux; manual QA checklist per milestone |
| Tauri major version churn (v3 in alpha) | Upgrade cost | Pin Tauri 2.x; thin glue crate limits blast radius |

## 11. Core seams for the parity roadmap

The parity addendum adds ~15 milestones of advanced capabilities. They are
**not** implemented now, but a few core components are shared by almost all
of them; getting their interfaces wrong would force rewrites. Each is
designed (interface + ADR) in the milestone that first needs it, before the
second consumer appears.

| Component | First needed | Consumers | Decision |
|---|---|---|---|
| **Version-agnostic message model** | M2 (HTTP/2 via ALPN) | every tool, storage, viewer | ADR-0009: raw bytes stay the source of truth for HTTP/1.x; HTTP/2 stores the ordered header list incl. pseudo-headers, stream id and protocol metadata. `CapturedExchange` gains a `protocol` field; the DB a `protocol_meta` blob. |
| **Request Engine** | M4 (Repeater) | Repeater, Fuzzer, Crawler, Scanner, Discovery, plugins, ChosAgent | ADR-0009: one crate (`crates/request`) owning DNS, TCP, TLS, HTTP/1.1, HTTP/2, upstream/SOCKS, pooling, redirects, cancellation, timing. Every send passes a `SendContext { source, task, resource_pool, scope_check }`. The proxy's upstream side moves onto it. |
| **Scope engine** | M5 | all automated tools, Live Tasks, scheduler, agent | Pure function `Scope::check(&Target) -> ScopeDecision` in `core`; the Request Engine refuses out-of-scope sends from automated sources by default. |
| **FilterEngine** | M5 | History, Logger, Site Map, WebSockets, Scanner, Organizer | One condition model compiled to SQL where possible and evaluated in memory otherwise; UI filter bars are built from the same model; ChosScripts filters (M22) become one more condition type. |
| **Task system + resource pools** | M10 | Crawler, Fuzzer, Scanner, Discovery, imports/exports, scheduler, agent tasks | Tasks have the spec states (Queued … Cancelled), structured event logs and counters; every automated send acquires a permit from its task's pool (concurrency, rate, delay, jitter, retries). M10 ships the limiter; M26 completes pools. |
| **Findings store** | M9 | Scanner, ChosChecks, OAST, DOM Inspector, plugins, reporting, AI | Issue + multiple evidence items + affected locations + configurable fingerprint + review status; suppressions are records, not deletions. |
| **Permission model** | M14 | Plugins, ChosScripts, REST API, ChosAgent | ADR-0010: a single capability vocabulary (`http.read`, `http.modify`, `http.send`, `project.read`, `project.write`, `finding.create`, `scope.modify`, `ui.extend`, …) with grants `DENY / ASK / ALLOW`, enforced in the engine API layer, never by the caller's goodwill. |
| **Settings layers** | M8 | everything configurable | User sections (today's TOML files) + project overrides stored in the project; effective value = project override ?? user value; import/export as validated JSON with a preview diff. |
| **Project audit log** | M8 | scope changes, plugins, scans, AI approvals | Append-only table in the project DB written by the Engine. |

What is deliberately **not** done now: no empty crates or traits for future
tools, no plugin runtime choice (decided in M14 with an ADR), no AI provider
abstraction (M29).

### Current code already aligned with these seams

* Every stored exchange carries `source` (proxy/repeater/fuzzer/…), as
  Logger, Live Tasks and the AI activity log require.
* `CaptureSink` is the hook where Intercept, site map, passive analysis,
  Live Tasks and plugins attach without the proxy knowing them.
* `Engine` is UI-agnostic: the CLI, REST API and agent tool layer are new
  adapters over it, not rewrites.
* Configuration is already split into independent, validated sections.

### Known refactor ahead (planned, small)

Done (M4, first step): the proxy's upstream connector now lives in
`crates/request` (`Upstream`, `UpstreamSecurity`). The send path for manual
tools (Repeater) is the next addition there, so there is never a second HTTP
implementation.
