# Development

## Toolchain

* Rust stable ≥ 1.89 (`rust-toolchain.toml` pins the stable channel with
  rustfmt and clippy).
* Node.js ≥ 22 and pnpm (version pinned in `package.json#packageManager`).
* Linux: WebKitGTK 4.1 development packages (see README).

## Common commands

| Command | What it does |
|---|---|
| `pnpm install` | Install frontend dependencies |
| `pnpm dev` | Run the desktop app with hot reload (Vite + Tauri) |
| `pnpm build` | Release build and Linux bundles (`.deb`, AppImage) |
| `pnpm test` | All Rust tests, then frontend tests |
| `pnpm lint` | ESLint + `cargo clippy -D warnings` |
| `pnpm format` / `pnpm format:check` | Prettier + rustfmt |
| `pnpm check` | Everything CI runs, in order |
| `cargo test --release -p chosproxy-app --test perf -- --ignored --nocapture` | Performance measurements (large project, proxy under load); see docs/PERFORMANCE.md |
| `pnpm lab --port 9090 --tls-port 9443` | Local lab server (HTTP + self-signed HTTPS, WebSocket echo at `/ws`, demo page `/ws-demo`) for manual testing |
| `scripts/build-kali.sh` | `.deb` + AppImage for Kali x86_64, built in a Kali container (needs Docker; on macOS Colima with Rosetta) → `dist-kali/` |
| `scripts/smoke-test-kali-deb.sh dist-kali/*.deb` | Installs the `.deb` in a clean Kali container and starts it under Xvfb |
| `scripts/make-macos-dmg.sh` | Ad-hoc-signed `.dmg` from the release `.app` (hdiutil only, no Finder scripting) |
| `scripts/make-backup.sh kali\|mac` | Self-contained zip per platform: ready-to-use app + full source with git history → `backups/` |

`cargo test` also regenerates the TypeScript types in
`apps/desktop/src/bindings/` from Rust (`ts-rs`). Commit them;
`scripts/check-bindings.mjs` fails CI if they are stale. Never edit them by
hand.

The Tauri crate embeds the built frontend, so run
`pnpm --filter @chosproxy/desktop build` once before `cargo clippy` /
`cargo build` of the whole workspace.

## Isolated runs

`CHOSPROXY_HOME=/some/dir` puts configuration, logs, data and temporary
projects under one directory — use it for manual testing so your real
settings are untouched. `CHOSPROXY_LOG=debug` (or any `tracing` filter)
overrides the log level.

## Layout

See [ARCHITECTURE.md](ARCHITECTURE.md). In short:

```
crates/core      config, branding, events, captured-exchange type, redaction
crates/http      wire-exact HTTP/1.1 codec
crates/tls       local CA, per-host certificates, upstream TLS
crates/storage   SQLite project database
crates/proxy     listeners and relay
crates/app       UI-agnostic Engine
crates/lab       local test server
crates/oast      ChosOAST interaction server and the chosoast binary
crates/dom       DOM Inspector instrumentation and DevTools client
crates/discovery content discovery candidates and soft-404 detection
apps/desktop     Tauri shell (src-tauri) + React UI (src)
```

Frontend structure: `features/` (screens), `components/` (reusable UI;
`components/message/` is the shared HTTP message viewer), `services/` (the
only code allowed to import Tauri), `state/` (Zustand stores), `lib/` (pure,
unit-tested helpers).

## Testing strategy

* **Unit tests** next to the code (parsers, framing, scope of config
  validation, storage queries, frontend helpers).
* **Integration tests** (`crates/*/tests/`) drive real TCP traffic through
  the proxy to the hyper-based lab server and to scripted servers for edge
  cases (close-delimited bodies, 1xx, malformed upstreams, stale keep-alive).
* No test depends on external websites.

## Renaming the product

The user-visible name lives in `branding.json`. Also update `productName`,
`mainBinaryName` and `identifier` in `apps/desktop/src-tauri/tauri.conf.json`
(a unit test fails if `productName`/`identifier` disagree with
`branding.json`). Crate and package names (`chosproxy-*`) are internal and do
not need to change.

## Conventions

* Commits: Conventional Commits (`feat(proxy): …`), small and focused.
* Rust: typed errors, no `unwrap()` on runtime data (clippy denies
  `unwrap_used`), doc comments on public items, `unsafe` forbidden.
* TypeScript: strict, no `any`, no `dangerouslySetInnerHTML` (lint error).
* UI: every view has loading, empty and error states; no control is shown
  unless it works.
