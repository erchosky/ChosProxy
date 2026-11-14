# ADR-0032: ChosScripts and ChosChecks (sandboxed scripts)

* Status: Accepted
* Date: 2026-11-12

## Context
The parity matrix asks for scripts in several places:
* filters and columns in the history;
* scripted Match & Replace;
* custom actions;
* a library of scripts;
* a language for custom checks.

The roadmap planned WASM-sandboxed scripting. Testers write these scripts
by hand, often in a hurry and for one engagement, so they need a small
language they can type directly. That language must still be unable to
touch the machine or hang ChosProxy.

## Decisions
1. **Rhai instead of WASM.**
   * Scripts are written in [Rhai](https://rhai.rs), embedded through the
     `chosproxy-script` crate. Rhai is a small scripting language
     implemented in Rust.
   * The engine starts empty and gets only the standard package: strings,
     arrays, maps and maths.
   * It has no file, network, process or environment access. `eval` is
     disabled and modules are refused.
   * Every run is bounded: 2 000 000 operations, 16 MB strings, 100 000
     items per array or map, and a limited call and expression depth.
   * This gives the same isolation WASM was chosen for, without asking
     testers to compile anything. A failing or runaway script only produces
     an error.
2. **Scripts see one message, read-only.**
   * A script gets `msg` with its method, URL, host, port, path, status,
     MIME, protocol, the raw request and response, decoded bodies and
     header lookups.
   * Helpers: regular expressions (size-limited), Base64 and a
     case-insensitive `contains_ci`.
   * Rewrite scripts receive and return the message text only.
3. **Five kinds, one entry point each.**

   | Kind | Entry point | Used by |
   |---|---|---|
   | check | `fn check(msg)` | passive analysis (findings) |
   | filter | `fn filter(msg)` | HTTP history and Logger |
   | column | `fn column(msg)` | HTTP history and Logger |
   | action | `fn action(msg)` | every request menu |
   | rewrite | `fn request(text)` / `fn response(text)` | Match & Replace rules |

4. **The library is application configuration** (`scripts.toml`), so
   scripts work in every project.
   * Scripts are compiled once when the library is saved. Scripts that do
     not compile are kept and reported with their line, and they never run.
   * Only enabled scripts run.
   * Editing happens in Tools → Scripts, with templates and test runs on a
     stored exchange. A test run never saves or sends anything.
5. **Integration.**
   * Check findings are merged like built-in passive issues (`passive`
     source, a key per script and finding).
   * Script filters check the newest 20 000 rows that match the other
     filters once, then page from that result. The result is cached until
     the filter, the library or the traffic changes.
   * Rewrite rules buffer the whole message like body rules. They decode
     it first and fix `Content-Length`. Messages that are not text are left
     alone, with a note.

## Consequences
* One sandbox serves every kind of script, and its limits are tested.
* Scripted filters cost one pass over at most 20 000 stored exchanges; new
  traffic triggers another pass when a filter is active.
* Deferred, explicitly, to **M22b**:
  * WebSocket messages in Match & Replace;
  * importing and exporting scripts;
  * an action button in Repeater;
  * a filter condition builder;
  * regular-expression Intercept and passthrough rules;
  * scripted payload processing in the Fuzzer.
* Checks that send requests belong to the active scanner (M25).
