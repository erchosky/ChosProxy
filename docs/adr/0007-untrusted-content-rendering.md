# ADR-0007: Never render untrusted content in the privileged WebView

* Status: Accepted
* Date: 2026-10-21

## Context
Everything ChosProxy displays may be attacker-controlled. The WebView has IPC
access to the engine (send traffic, read project files). XSS in the UI would
be a full compromise of the testing workstation's project data.

## Decision
* Traffic is displayed as **text** (CodeMirror documents / React text nodes).
  `dangerouslySetInnerHTML` and `eval` are banned by lint rule.
* Strict CSP: `default-src 'self'`; no remote scripts; `connect-src` limited
  to Tauri IPC.
* The future "Render" view uses an `<iframe sandbox>` *without*
  `allow-scripts`/`allow-same-origin`, fed via `srcdoc`, with a CSP that
  blocks all network loads. Shipped in M6 (ADR-0014).
* Tauri capabilities grant only the commands the UI uses.

## Consequences
* ➕ Hostile responses cannot reach IPC.
* ➖ Rendered previews will be visually incomplete (no remote assets); this is
  an accepted trade-off.
