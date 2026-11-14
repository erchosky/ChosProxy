# ADR-0039: ChosAI

* Status: Accepted
* Date: 2026-11-12

## Context
M29 covers AI-001 to AI-004: "explain this", prompts in Repeater, finding
analysis and a login helper. The specification sets the rules:
* local models first-class, external providers opt-in;
* always show what will be sent to an external model;
* redact credentials;
* never send the whole project;
* no telemetry;
* the AI does no offensive actions.

Agentic features with tool permissions are M30.

## Decisions
1. **Text only.** ChosAI returns explanations, suggested manual checks,
   session-rule suggestions and report wording. It has no tools: it does
   not send traffic or change the project. Its system prompt also asks it
   not to write exploit code.
2. **Modes in `ai.toml`.**
   * The modes are Disabled (the default), Local and External.
   * Local mode only accepts a loopback URL. External mode needs https,
     unless the URL is loopback.
   * Two API styles cover the common servers: Ollama `/api/chat`, and
     OpenAI-compatible `/v1/chat/completions` (LM Studio, llama.cpp,
     vLLM and hosted services). There are no vendor SDKs.
   * An API key is a keyring credential, so it is never stored in
     `ai.toml`.
3. **Context is built from attachments, then redacted, then limited.**
   * Attachments are exchanges, findings with their first evidence, or
     editor text; at most 20 per question.
   * Redaction runs in `chosproxy-ai`, which is pure code. It covers
     headers, cookies, JWTs, secret-named parameters and JSON fields,
     known key formats and custom patterns. It reports what it hid.
4. **The preview is enforced by the engine, not only by the UI.**
   * `ai_preview` returns the exact messages and their SHA-256 digest.
   * `ai_run` toward a model that is not on this machine is refused unless
     it carries that digest. The engine recomputes the digest from the
     same inputs, so a changed question or attachment, or a stale preview,
     is refused.
5. **The Request Engine sends the calls,** so upstream proxies and TLS
   settings apply. The calls are not stored in the history. The audit log
   records the task, the number of items, the size and the model, never
   the content.
6. **Entry points in the UI:** the request menu, the finding editor,
   Repeater, and message viewer selections, all through one dialog. The
   dialog has a task, a question, the preview and the answer as Markdown.

## Consequences
* With a local model, testers get explanations without data leaving the
  machine.
* External providers are possible, with a forced look at what leaves.
* AI-004 is a helper rather than a recorder: it explains a login across
  requests the tester picks and proposes a session rule as text.
* Deferred, explicitly:
  * streaming answers, conversation history and an AI activity log → M30;
  * ChosAgent (tools with DENY/ASK/ALLOW) → M30;
  * applying a suggested session rule in one click → M30;
  * model plugins → M31.
