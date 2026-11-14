# ChosProxy

**A modern, open-source web security testing workbench for Kali Linux** — an
intercepting HTTP(S) proxy and toolkit for inspecting, modifying and
understanding web application traffic.

> ⚠️ **Authorized use only.** Use ChosProxy only against systems you own or
> are explicitly authorized to test: local labs (OWASP Juice Shop, DVWA,
> WebGoat, PortSwigger Web Security Academy), CTFs, your own development
> environments, or engagements covered by written permission. Testing other
> systems may be illegal. See [SECURITY.md](SECURITY.md).

ChosProxy is built from scratch (Rust engine, Tauri + React UI). It is
inspired *conceptually* by the problems professional suites solve, with its
own code, design and identity. Everything runs locally: no telemetry, no
analytics, no cloud.

## Status

Early development — see [ROADMAP.md](ROADMAP.md).

| Available now | Next |
|---|---|
| Temporary and persistent projects (single SQLite file) | M16: performance and hardening |
| HTTP proxy listeners (start/stop/restart, multiple, loopback by default), HTTP/2 to browsers, invisible mode for non-proxy-aware clients | M17: 1.0 packages for Kali |
| Byte-exact HTTP/1.1 relay (keep-alive, chunked, 100-continue, 1xx, upgrades) | |
| Intercept: pause, edit, forward or drop requests and responses, with rules | |
| Match & Replace: automatic changes to request and response lines, headers and bodies | |
| Network settings: upstream HTTP/SOCKS5 proxies per destination, host name overrides, timeouts (docs/NETWORK.md) | |
| Repeater: tabs and groups saved in the project, send/cancel, group sends (sequence, single connection, parallel), kept connections, cookie jar, timing, per-tab history, follow redirects (by hand or automatically), notes per tab, collapsible groups, HTTP/2 and single-packet race tests | |
| Shared right-click menu to send requests to other tools; a menu inside the message editors with copy as curl, Python or fetch | |
| Passive analysis: 25 checks (single requests, JavaScript, and across requests such as a session not renewed at login) find weaknesses in traffic as you browse, with explanations; suppression rules for false positives | |
| Scans: crawl, passive audit, JavaScript analysis, and optional active audit (off by default; requires explicit target, non-empty scope, per-launch authorization confirmation and request budgets; built-in non-destructive XSS/SQLi/redirect/CORS/OAST checks plus custom active check scripts), with configurations you can save, share and schedule, a dashboard, a check picker, and a log and request notes that explain every decision | |
| Live tasks: a passive audit or passive crawl that follows your traffic as it arrives and carries on when you reopen the project | |
| M31b: streamed ChosAI answers, plugin hooks for responses and HTTP/2, a certificate per listener, NTLMv2, keyring in the macOS Keychain or behind a master password, an SMTP sink for ChosOAST, crawl graph, nested Organizer collections, saved tool settings, WebSocket scripts, an OpenAPI description and event stream for the API | |
| Resource pools: shared request limits for the crawler, Fuzzer, Sequencer, Repeater and plugins, chosen per task at launch or while it runs, slowing down when a server answers 429/503 | |
| ChosAI: explanations, finding analysis and report text from a local model (or an opt-in provider), with redaction and a context preview (docs/AI.md) | |
| ChosAgent: an agent that uses ChosProxy's own tools under Deny / Ask / Allow permissions enforced by the engine (requests always approved, in scope), with an AI activity log (docs/AI.md) | |
| Encrypted project files (SQLCipher), crash recovery with read-only inspection mode, password-sealed Organizer and settings sharing, local Security & Privacy posture center, filtered project audit log, temporary-file cleanup and a local Performance view (docs/PROJECT_FILES.md) | |
| Automation API and `chosproxy-cli`: headless mode and scripting on 127.0.0.1 with a token (docs/API.md) | |
| API workspace: OpenAPI/Swagger, Postman and WSDL definitions as requests for Repeater and the Fuzzer, bounded sends; GraphQL introspection, schema browser and query builder | |
| Crawler: maps a site in the background (GET only, in scope), with pausable tasks; browser-powered crawl through the lab browser | |
| Sessions: encrypted credentials, Basic/Digest platform authentication, session rules with login macros and variables | |
| Fuzzer: payload positions (auto-marked in query, form, cookies, JSON, multipart, XML), four attack modes, payload processing (incl. dates), grep match/extract, stop conditions, CSV export, highlighted results | |
| Sequencer: capture tokens and see how random they are, with explained statistics | |
| Plugins (APIs v1 and v2): separate programs with granted permissions — traffic, findings, sending requests, project queries, menu actions, request hooks that change, block or answer in-scope proxy requests; signed plugins and trusted publishers; see PLUGIN_SDK.md | |
| Scripts (sandboxed): your own checks, history filters and columns, actions and Match & Replace rewrites; see docs/SCRIPTS.md | |
| Engagement tools: content discovery with soft-404 detection, CSRF PoC, FrameLab (clickjacking), target analyzer, find references, site map snapshots, scheduler | |
| DOM Inspector: source → sink analysis in the lab browser (Chromium family), with findings (docs/DOM_INSPECTOR.md) | |
| OAST: out-of-band DNS/HTTP(S) interactions linked to the requests that caused them; local or self-hosted `chosoast` server (docs/OAST.md) | |
| HTTPS interception with a local CA generated on your machine (or one you import) | |
| Isolated lab browser (Chromium or Firefox) — one click, no trust-store changes | |
| HTTP history: virtualized table, filters (incl. MIME type), sorting by column, request/response viewer (Pretty / Raw / Hex with byte selection / Inspector, with editable JSON fields), saved filters, copy as curl | |
| Dashboard with a getting-started checklist, settings, command palette (Ctrl/Cmd+K, also finds stored requests), keyboard shortcuts you can rebind (`?` lists them), scope import from a list of hosts or URLs, dark/light/system themes | |

## Install on Kali Linux (x86_64)

```bash
sudo apt install ./chosproxy_1.0.0_amd64.deb
```

Then start **ChosProxy** from the applications menu or run `chosproxy`.
Alternatively make the AppImage executable and run it. Packages are built
with `scripts/build-kali.sh` (or downloaded from CI artifacts).

## Quick start (from source)

Requirements: Rust (stable, ≥ 1.89), Node.js ≥ 22, pnpm, and on Linux the
WebKitGTK development packages:

```bash
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev librsvg2-dev libxdo-dev build-essential
```

```bash
pnpm install
```

```bash
pnpm dev
```

Then click **Start temporary project**, then **Open browser**. The proxy
listens on `127.0.0.1:8080`; the lab browser is an installed Chromium/Chrome
or Firefox started with a temporary profile that already uses the proxy and
accepts ChosProxy's HTTPS certificates. Nothing is installed in your system
or in your everyday browser profile. On Kali, if no browser is detected:

```bash
sudo apt install chromium
```

### HTTPS and the local CA

On first start ChosProxy generates a certificate authority on your machine
(Proxy → **HTTPS & browser** shows it). Its private key never leaves your
machine. The lab browser trusts it automatically for its temporary profile.
For another dedicated testing browser, open `http://chosproxy/` through the
proxy and import the certificate (see the in-app instructions).

### Try it with OWASP Juice Shop

```bash
docker run --rm -p 127.0.0.1:3000:3000 bkimminich/juice-shop
```

Browse `http://localhost:3000` in the lab browser; requests appear in
**Proxy → HTTP history**.

### Built-in lab server

A deliberately simple local server (loopback only) for exercising the proxy,
on HTTP and on HTTPS with a self-signed certificate:

```bash
pnpm lab -- --port 9090 --tls-port 9443
```

## Documentation

| Document | Contents |
|---|---|
| [docs/PROJECT_SPEC.md](docs/PROJECT_SPEC.md) | Product specification (+ [parity addendum](docs/PROJECT_SPEC_ADDENDUM_PARITY.md)) |
| [docs/BURP_PARITY_MATRIX.md](docs/BURP_PARITY_MATRIX.md) | Capability parity matrix vs. the reference suite |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Crates, data flows, interfaces, core seams, risks |
| [ROADMAP.md](ROADMAP.md) | Milestones and their exit criteria |
| [DEVELOPMENT.md](DEVELOPMENT.md) | Building, testing, project layout, conventions |
| [THREAT_MODEL.md](THREAT_MODEL.md) | Security model of ChosProxy itself |
| [SECURITY.md](SECURITY.md) | Intended use, vulnerability reporting |
| [CONTRIBUTING.md](CONTRIBUTING.md) | How to contribute |
| [PLUGIN_SDK.md](PLUGIN_SDK.md) | Plugin API design (Milestone 14) |
| [docs/DOM_INSPECTOR.md](docs/DOM_INSPECTOR.md) | DOM Inspector: client-side source → sink analysis |
| [docs/OAST.md](docs/OAST.md) | ChosOAST: local and self-hosted out-of-band testing |
| [docs/adr/](docs/adr/) | Architecture decision records |

## License

Not yet selected — the options and a recommendation are documented in
[ADR-0008](docs/adr/0008-license.md) and await the project owner's decision.
