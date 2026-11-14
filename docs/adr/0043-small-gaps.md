# ADR-0043: M33, small gaps

* Status: Accepted
* Date: 2026-11-14

## Context
After the M32 parity audit, several partial rows were missing one small
piece each. None of them changes what ChosProxy sends to a target: no new
traffic, no attack payloads, no active checks (M13 remains pending).

## Decisions
1. **Collapsed Repeater groups are saved in the project**
   (`repeater_groups.collapsed`, schema v28), like the group's name and
   colour. A collapsed group shows its chip, its tab count and the
   selected tab only.
2. **Out-of-scope proxy traffic can be left unstored, not dropped.** The
   option lives in the scope configuration (`skip_out_of_scope_proxy`), so
   a project with its own scope has its own choice. The proxy still
   forwards the traffic; the project sink does not store it (history,
   WebSocket connections whose handshake was out of scope, cookie jar).
   Dropping it would make the browser show failures that are not the
   target's, so it is not offered. The option does nothing while no scope
   is defined.
3. **Fuzzer stop conditions are counted on the attack's own requests.** The
   baseline does not count. Stopping ends the task as completed and the log
   says which condition stopped it.
4. **The CSV export guards against formulas.** Payloads and extracts come
   from the tested application; a field starting with `=`, `+`, `-`, `@`,
   tab or carriage return is prefixed with `'`. Highlight colours of
   results are stored in the project (schema v29). Hidden result columns
   are a per-computer view preference (browser storage), not project data.
5. **The event log is a private JSON Lines file** in the log directory
   (`0600`), compacted to the last 2,000 entries at start. Clear empties
   the file. If the file cannot be written, the log stays in memory and
   says so. It holds only the text the event log shows (notices and
   errors), never request or response contents.
6. **A pool chosen at launch is a task-local of the start call.**
   `Engine::with_launch_pool` checks the name, then runs the start call
   with it in scope; the task takes it when it is registered, before its
   first request. The start commands take an optional `pool`.

## Consequences
Seven matrix rows become complete (94 / 34 / 4 / 2). Parity is still not
declared: the rows listed in the M32 audit remain.
