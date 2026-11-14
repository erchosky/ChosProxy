# ADR-0010: One permission model for plugins, scripts, API and agent

* Status: Accepted (implementation from M14)
* Date: 2026-10-22

## Context
Plugins (M14/M23), ChosScripts (M22), the automation REST API (M28) and
ChosAgent (M30) all act on the project and the network on the user's
behalf. Four ad-hoc permission systems would drift and leave gaps.

## Decision
A single capability vocabulary in `chosproxy-core` (e.g. `http.read`,
`http.modify`, `http.send`, `project.read`, `project.write`,
`finding.create`, `scope.modify`, `task.start`, `ui.extend`) and grants with
three values: `DENY`, `ASK`, `ALLOW`. Grants are stored per principal
(plugin id, script id, API token, agent profile). Enforcement happens inside
the Engine API that performs the action — the caller cannot bypass it — and
every `ASK` decision and privileged action is written to the project audit
log.

## Consequences
* ➕ Uniform UI for reviewing what any automation may do (Security Center).
* ➕ Agent permissions do not depend on model behaviour.
* ➖ Engine APIs need a principal parameter; added when M14 starts, not now.
