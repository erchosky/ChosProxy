# ADR-0040: ChosAgent

* Status: Accepted
* Date: 2026-11-12

## Context
M30 covers AI-005 (agentic testing with approval controls) and AI-006
(the AI activity log), plus what M29 deferred: conversations, applying a
suggested session rule, streaming. The specification (§107–§108) asks for:
* only ChosProxy's deterministic tools, no arbitrary OS access;
* per-tool DENY / ASK / ALLOW, enforced by the tool layer, not by the
  prompt;
* tasks with subtasks, queued messages, pause, stop and resume;
* an activity log (action, tool, time, target, permission, result), and
  the AI's requests visible in the Logger.

ChosAgent and the active audit framework are separate components. M13
was selected next by the user on 2026-11-14 and remains unimplemented;
this ADR describes the existing M30 agent catalog and permissions.

## Decisions
1. **A fixed catalog is the agent's whole world.** `chosproxy_ai::agent`
   lists twelve tools: reads (history search, a request, findings, a
   finding, scope, passive checks on a stored request, the Decoder), the
   task's plan, project changes (an exchange note, a tentative finding, a
   session rule) and one sender, `send_request`. There is no shell, file,
   browser or plugin access. A name outside the catalog is refused and
   recorded.
2. **One dispatcher enforces permissions.** Every call goes through
   `Engine::agent_dispatch`: catalog check → effective permission →
   argument check (`agent_prepare`) → approval when ASK → run. Each tool
   has a default and a highest permission; `effective()` caps whatever
   `agent.toml` says, and saving a setting above the cap is refused. Denied
   tools are also not offered to the model, but the dispatcher does not
   rely on that.
3. **Sending is always asked, scoped and budgeted.**
   * `send_request` has highest permission ASK: no setting lets it run
     without the tester approving the exact request text, shown with the
     agent's reason.
   * It needs a *defined* scope containing the target (stricter than
     Repeater, which accepts any target while no scope exists); the scope
     is checked before asking and again before sending.
   * A per-task budget (`maxSends`, default 20) and a step limit
     (`maxSteps`, default 25).
   * Requests go through `session_send` (resource pools, session rules,
     tool name `agent`) and are stored with source `agent` and a note
     naming the task, so they show in HTTP history and the Logger.
   * There are no payload lists, active checks or scan launches among the
     tools, and the system prompt forbids attack payloads, brute force and
     load. Active audit belongs to M13, which remains pending.
4. **External models: every context is approved.** With a model outside
   this machine, each model call waits for the tester to approve the new
   messages (the whole conversation on the first call), keeping M29's rule
   that nothing leaves unseen. The conversation digest is checked between
   approval and call.
5. **Results are redacted and cut** with ChosAI's rules before the model
   sees them; old tool results are dropped first when the conversation
   grows past four times the context limit.
6. **Models without native tool calling** get the tools described in the
   prompt and call them with a fenced `tool` JSON block. When a server
   refuses `tools`, the agent switches to that mode for the task.
7. **Tasks** reuse the task framework (kind `agent`): pause, resume and
   stop are the task controls; stopping while an approval waits ends the
   task. Tester messages are queued and read before the next model call;
   `update_plan` keeps the subtasks. Agent tasks are not resumed after the
   project is reopened (the conversation lives in memory).
8. **The AI activity log** (schema v25, `ai_activity`) records the goal,
   each model call, each tool call with permission and decision, tester
   messages, approvals of outgoing context, the answer, stops and errors.
   ChosAI one-shot questions add a row too, without their content. Rows go
   with their task. The UI's transcript is built from these rows.
9. **Approvals are UI-only.** The automation API (M28) does not expose
   agent tasks or approvals: a script cannot approve an agent's request.
10. **M29 deferrals.** ChosAI questions become conversations (earlier
    turns are sent again, the tester's turns redacted, all inside the
    preview digest). The login-flow answer ends with a
    `chosproxy-session-rule` JSON block that the dialog opens in Project →
    Session rules as an unsaved draft for review. Streaming answers stay
    deferred: the Request Engine reads whole responses, and a streaming
    path is not worth a second sender here.

## Consequences
* A tester can hand a goal to an agent and keep control: reads run, every
  request waits for approval, and everything is in the activity log.
* Small local models without tool calling still work, less reliably.
* The agent's answers come from a model and are marked as such.
* Deferred, explicitly:
  * streaming answers → M31;
  * recording a login in the lab browser for AI-004 → M31;
  * agent tasks surviving a project reopen → M31;
  * model providers through plugins → M31 (from M29);
  * agent access over the automation API: not planned without a decision.
