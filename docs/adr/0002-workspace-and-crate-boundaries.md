# ADR-0002: Workspace layout and crate boundaries

* Status: Accepted
* Date: 2026-10-21

## Context
The spec proposes ~18 crates up front and a separate `packages/ui`. We want
modularity without a monolith, but also no empty scaffolding.

## Options
1. Create every crate now with stub APIs.
2. **Create crates when their milestone begins**, with boundaries documented
   in ARCHITECTURE.md; add a UI-agnostic `app` crate as composition root.
3. A single core crate with modules, split later.

## Decision
Option 2. Crates in M0/M1: `core`, `http`, `storage`, `proxy`, `app`, `lab`,
plus the Tauri glue crate in `apps/desktop/src-tauri`. The UI stays inside
`apps/desktop/src` until a second consumer exists.

## Consequences
* ➕ Every crate that exists has real code and tests; no fake modules.
* ➕ `app::Engine` lets a future CLI/headless mode reuse everything.
* ➖ Adding a crate later touches the workspace manifest (trivial).
* ➖ Extracting `packages/ui` later is some work; mitigated by keeping all
  Tauri calls in `src/services/`.
