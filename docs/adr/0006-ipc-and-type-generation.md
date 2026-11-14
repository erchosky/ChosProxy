# ADR-0006: IPC via Tauri commands/events, TS types generated with ts-rs

* Status: Accepted
* Date: 2026-10-21

## Context
UI and engine exchange many typed structures; drifting hand-written types are
a classic source of bugs.

## Options
1. Hand-written TS interfaces.
2. **`ts-rs` derive** — TS declarations generated from Rust types during
   `cargo test`, committed to `apps/desktop/src/bindings/`, checked in CI.
3. `specta`/`tauri-specta` — also generates command wrappers, but tighter
   coupling to Tauri versions and still-evolving API.

## Decision
Option 2. Commands are thin and return `Result<T, IpcError>`. Engine events
are coalesced in the glue layer (max 5 Hz) before emission. Binary payloads
are sent base64-encoded with explicit size caps.

## Consequences
* ➕ Single source of truth for types; CI fails if bindings are stale.
* ➖ Command names/arguments are still hand-wired in `services/`; kept in one
  file per area so drift is easy to review.
