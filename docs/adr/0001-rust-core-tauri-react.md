# ADR-0001: Rust core, Tauri 2 shell, React + TypeScript UI

* Status: Accepted
* Date: 2026-10-21

## Context
ChosProxy needs a fast, memory-safe network engine that handles hostile
input, and a modern desktop UI that runs well on Kali Linux (x86_64 first).

## Options
1. **Rust core + Tauri 2 + React/TS** — native engine, system WebView
   (WebKitGTK on Linux), small binaries, strong capability-based IPC security.
2. Rust core + Electron — mature, consistent Chromium rendering, but ~150 MB
   per install, larger attack surface, Node in the main process.
3. JVM (Kotlin/Java + Swing/JavaFX) — proven for this tool category, but
   heavier runtime and a dated UI toolkit; contradicts the spec stack.
4. Rust core + native Rust UI (egui/iced/Slint) — single language, but
   immature rich-text/code editors and table virtualization, slower UI dev.

## Decision
Option 1. Tauri is pinned to the **2.x** line (3.x is alpha at time of
writing).

## Consequences
* ➕ Memory-safe parsers, async I/O (Tokio), small install, `.deb`/AppImage
  bundling built in, fine-grained IPC permissions.
* ➕ Web tech gives fast iteration for complex views (virtual tables, editors).
* ➖ WebKitGTK quirks on Linux; must test on Debian-family distros in CI.
* ➖ Two languages; mitigated with generated TS bindings (ADR-0006).
