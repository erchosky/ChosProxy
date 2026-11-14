# ADR-0025: Plugins (API v1)

* Status: Accepted
* Date: 2026-11-11

## Context
Milestone 14 adds plugins (spec §36–37): plugins receive HTTP events,
analyze requests and responses and add findings, with declared permissions
that are never granted automatically and execution isolated as far as
possible, plus a documented SDK and a minimal `HelloPlugin`. ADR-0010 set
one permission model for plugins, scripts, the API and the agent, enforced
inside the engine and audited.

## Decisions
1. **A plugin is a separate process speaking JSON lines** over standard
   input and output (`chosproxy-plugin`: manifest, permissions, messages).
   Any language works, a crashing plugin cannot take ChosProxy down, and the
   engine sees every action as a message it can check. No WebAssembly
   runtime is available to the project yet; scripted checks in a WASM
   sandbox remain planned for M22.
2. **Isolation is stated honestly.** The process runs with the user's
   operating-system rights. ChosProxy limits what it can do through
   ChosProxy, starts it with a minimal environment (no inherited secrets),
   in its own folder, with output limits (1 MiB lines, 5 s write timeout,
   20 invalid messages, 120 findings per minute), and the UI says plainly
   that only trusted plugins should be installed.
3. **Permissions: three implemented, three reserved.** `network.read`,
   `scanner.passive` and `project.write` work in API v1; `network.send`,
   `project.read` and `ui.extend` are names reserved by the spec and a
   manifest asking for them is refused rather than accepted and silently
   ignored. The tester grants each permission (a subset is allowed); grants
   live in `plugins.toml`.
4. **Approval is tied to the files.** Enabling a plugin records the
   SHA-256 of its folder; if any file changes (interpreter caches excepted)
   it is not started again until approved. Enabling, disabling and every
   refused action go to the project audit log.
5. **Traffic is delivered from the history.** A running plugin with
   `network.read` follows `historyAppended` events and receives each new
   stored exchange once (headers, bodies as text cut at 256 KiB). Findings
   go through the automatic findings path (source `plugin`, merged by
   `plugin:<id>:<key>`, false positives respected); annotations through the
   history annotations.
6. **Lifecycle.** Enabled plugins start when a project opens and stop
   (`shutdown`, then kill after 2 s) when it closes or the plugin is
   disabled. Status, errors and a 500-line log per plugin are shown in the
   Plugins view.

## Consequences
* ➕ Plugins can add checks without touching ChosProxy, in any language,
  with a permission check on everything they do.
* ➖ No OS-level sandbox, no plugin-sent requests, no UI extensions, no
  signed distribution (ChosHub, M23) in API v1.
* ➖ A reading bug fixed while building it: lines from a plugin are read by
  a dedicated task, because reading a line is not cancel-safe and must not
  race with other events.
