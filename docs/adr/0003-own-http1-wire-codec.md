# ADR-0003: Own wire-exact HTTP/1.1 codec for the proxy path

* Status: Accepted
* Date: 2026-10-21

## Context
Priority #3 is traffic fidelity: history must show exactly what was sent and
received, and Repeater/Intercept must be able to send exactly what the user
typed — including unusual or malformed requests that are the point of a
security test. General-purpose HTTP libraries normalize: hyper lower-cases or
re-cases header names, reorders/merges, rejects invalid syntax, re-frames
bodies, and owns connection management.

## Options
1. **hyper 1.x** server + client — battle-tested, HTTP/2 support, but
   normalizes messages; header-case preservation is partial; cannot send
   malformed requests; hard to capture exact bytes.
2. **Own codec over Tokio streams**, using `httparse` (zero-copy, fuzzed,
   widely used) only to tokenize message heads, with our own RFC 9112 body
   framing that streams *raw framing bytes*.
3. Fork an existing proxy (e.g., port of mitmproxy concepts) — large,
   different language/runtime.

## Decision
Option 2 for HTTP/1.x. For HTTP/2 (M2+) use the `h2` crate behind the same
version-agnostic message model, since HTTP/2 framing is binary and fidelity
there means header-list fidelity, which `h2` preserves.

## Consequences
* ➕ Byte-exact history and forwarding; malformed messages can be relayed and
  inspected rather than rejected.
* ➕ Full control over buffering limits and streaming (memory safety under
  huge bodies).
* ➖ We own edge-case correctness (chunked trailers, 1xx, HEAD, 304,
  close-delimited, pipelining, `Expect`). Mitigation: exhaustive unit tests,
  lab-server integration tests, parser fuzzing in M16.
* ➖ CL/TE ambiguities must be handled deliberately: TE wins, conflict is
  flagged in the exchange notes, bytes are not silently rewritten.
