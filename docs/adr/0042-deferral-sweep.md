# ADR-0042: The M31b deferral sweep

* Status: Accepted
* Date: 2026-11-13

## Context
Earlier milestones parked many unrelated items on M31. M31 itself became a
hardening milestone, so those items moved to M31b. None of them adds a new
way to attack a target: the scope decision of every milestone since M25
stays (no attack payloads, no active checks, nothing sent that the tester
did not ask for).

## Decisions
1. **Streaming is a read mode of the Request Engine, for the model call
   only.** `send_streaming` hands body pieces to a callback while the whole
   response is still collected. ChosAI parses Ollama NDJSON and
   OpenAI-compatible SSE; a server that does not stream is read as before.
   A cancelled answer is kept nowhere and logged nowhere.
2. **An interrupted agent task is never resumed; it is continued by a new
   task.** The new task gets the same goal and items and a redacted summary
   of the old activity log. Every approval is asked again.
3. **"Record login" only marks the history.** ChosProxy does not drive the
   browser or replay anything: it attaches the proxy requests made between
   start and stop to the login-flow helper.
4. **One route table for the API.** The OpenAPI document is generated from
   it and tests check it against the router. The event stream has the same
   token, `Origin` and `Host` rules as every route, a bounded queue per
   client (a client that does not read is dropped) and a small limit on
   the number of streams.
5. **Response hooks and HTTP/2 hooks use the existing permission and
   rules** (`proxy.intercept`, a defined scope, 2 s, fail open, notes in
   the history). A plugin opts in to responses when it registers its hook.
   Plugins read a chunked body in one piece; a response nobody changes is
   forwarded byte for byte.
6. **A pool per task is chosen while the task runs.** The choice lives with
   the running task and is found through a task-local value, re-scoped
   where a task spawns its sends. It is not stored with the task.
7. **A listener may present its own CA or one chosen certificate.** A
   certificate that cannot be loaded keeps the listener from starting,
   rather than falling back to another certificate. The lab browser trusts
   the shared CA only.
8. **NTLMv2 only, with library hash functions.** MD4, MD5 and HMAC come
   from the RustCrypto crates; the module only arranges the messages and
   is checked against the example in [MS-NLMP]. NTLMv1, LM and Kerberos are
   not spoken. The three messages go on one connection, so it is not used
   for kept Repeater connections or HTTP/2.
9. **The keyring key can live in three places:** a private file (default),
   the operating system's keychain (macOS now), or nowhere — derived from
   a master password (PBKDF2-HMAC-SHA256, 600 000 rounds) and held in
   memory for the session. The protection is recorded inside the keyring
   file, so a change is one atomic write. Tests use a keychain in memory
   and never touch the tester's.
10. **The SMTP listener is a sink.** No relay, no delivery, no AUTH, no
    STARTTLS, bounded commands, recipients and size; off by default.
11. **Nested collections are names with `/`** (no schema change). A colour
    per Repeater tab is a new column (schema v26).
12. **Site map conditions reuse the history's filter script path** and its
    bound on the requests looked at.
13. **The settings library stores settings only**: never a target, a start
    URL or a request. Loading puts values into a form; nothing starts.
14. **WebSocket scripts are one more entry point of rewrite scripts**
    (`fn websocket(text, to_server)`), run through a Match & Replace rule
    on text messages; a failing script leaves the message unchanged.

## Consequences
* New dependencies: `md4`, `hmac` (NTLM); `security-framework` on macOS.
* Not done and waiting for a decision: Secret Service on Linux, ACME,
  providers through plugins, INSTR-001, raw HTTP/2 frames (see ROADMAP
  "Milestone 31b — Deferred").
* Nothing here was seen in the running app; the real macOS Keychain, a
  real NTLM server and a real local model were not tried.
