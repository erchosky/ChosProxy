# Architecture Decision Records

Format: Context → Options → Decision → Consequences (pros/cons).
Status values: Proposed · Accepted · Superseded by NNNN.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-rust-core-tauri-react.md) | Rust core, Tauri 2 shell, React + TypeScript UI | Accepted |
| [0002](0002-workspace-and-crate-boundaries.md) | Workspace layout and crate boundaries | Accepted |
| [0003](0003-own-http1-wire-codec.md) | Own wire-exact HTTP/1.1 codec instead of hyper for the proxy path | Accepted |
| [0004](0004-sqlite-project-storage.md) | SQLite per project, single writer thread, compressed blobs | Accepted |
| [0005](0005-configuration-files.md) | Configuration split into per-section TOML files | Accepted |
| [0006](0006-ipc-and-type-generation.md) | IPC via Tauri commands/events, TS types generated with ts-rs | Accepted |
| [0007](0007-untrusted-content-rendering.md) | Never render untrusted content in the privileged WebView | Accepted |
| [0008](0008-license.md) | Open-source license selection | Proposed |
| [0009](0009-message-model-and-request-engine.md) | Version-agnostic message model and a single Request Engine | Accepted |
| [0010](0010-permission-model.md) | One permission model for plugins, scripts, API and agent | Accepted |
| [0011](0011-https-interception.md) | HTTPS interception with a per-install local CA | Accepted |
| [0012](0012-intercept.md) | Intercept — buffering, editing as text, decisions | Accepted |
| [0013](0013-scope-and-filters.md) | Scope engine, shared traffic filter and Target storage | Accepted |
| [0014](0014-utilities.md) | Utilities — Decoder, Comparer, Organizer, Render view | Accepted |
| [0015](0015-websockets.md) | WebSocket capture and interception | Accepted |
| [0016](0016-projects.md) | Projects — copies, backups, settings layers, audit log | Accepted |
| [0017](0017-findings.md) | Findings store | Accepted |
| [0018](0018-reporting.md) | Reporting — one model, escaped output, redacted evidence | Accepted |
| [0019](0019-repeater.md) | Repeater and the Request Engine send path | Accepted |
| [0020](0020-repeater-groups-connections-cookies.md) | Repeater groups, kept connections and the cookie jar | Accepted |
| [0021](0021-passive-analysis.md) | Passive analysis | Accepted |
| [0022](0022-tasks-and-crawler.md) | Tasks and the crawler | Accepted |
| [0023](0023-fuzzer.md) | The fuzzer | Accepted |
| [0024](0024-sequencer.md) | The sequencer | Accepted |
| [0025](0025-plugins.md) | Plugins (API v1) | Accepted |
| [0026](0026-oast.md) | ChosOAST (out-of-band interaction testing) | Accepted |
| [0027](0027-dom-inspector.md) | DOM Inspector | Accepted |
| [0028](0028-engagement-tools.md) | Engagement tools | Accepted |
| [0029](0029-network-and-match-replace.md) | Outgoing network settings, invisible listeners and Match & Replace | Accepted |
| [0030](0030-http2.md) | HTTP/2 in the Request Engine and the single-packet attack | Accepted |
| [0031](0031-http2-through-the-proxy.md) | HTTP/2 through the proxy | Accepted |
| [0032](0032-chosscripts.md) | ChosScripts and ChosChecks (sandboxed scripts) | Accepted |
| [0033](0033-plugin-api-v2-and-signing.md) | Plugin API v2 and signed plugins (ChosHub) | Accepted |
| [0034](0034-sessions-keyring-browser-crawl.md) | Keyring, platform authentication, session rules and browser crawling | Accepted |
| [0035](0035-scanner-engine.md) | Scanner engine: insertion points, fingerprints, scans and suppressions | Accepted |
| [0036](0036-resource-pools-live-tasks-hooks.md) | Resource pools, live tasks, cross-request checks and plugin hooks | Accepted |
| [0037](0037-api-graphql-workspace.md) | API and GraphQL workspace | Accepted |
| [0038](0038-automation-api-and-cli.md) | Automation API and `chosproxy-cli` | Accepted |
| [0039](0039-chosai.md) | ChosAI: local-first model access with redaction and enforced preview | Accepted |
| [0040](0040-chosagent.md) | ChosAgent: a fixed tool catalog with engine-enforced DENY/ASK/ALLOW, and the AI activity log | Accepted |
| [0041](0041-encrypted-projects-temp-files-performance.md) | Encrypted projects (SQLCipher), temporary files and the Performance view | Accepted |
| [0042](0042-deferral-sweep.md) | The M31b deferral sweep: streaming, hooks for responses and HTTP/2, per-listener certificates, NTLMv2, keyring protections, SMTP sink and the rest | Accepted |
| [0043](0043-small-gaps.md) | M33 small gaps: collapsed groups, unstored out-of-scope traffic, Fuzzer stop/export/colours/columns, a persistent event log, pools at launch | Accepted |
| [0044](0044-active-audit-and-m31-completion.md) | Active Audit Framework (M13) and M31 completion (crash recovery, encrypted sharing UI, Security Center and audit log coverage) | Accepted |
