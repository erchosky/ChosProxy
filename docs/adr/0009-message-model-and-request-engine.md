# ADR-0009: Version-agnostic message model and a single Request Engine

* Status: Accepted (implementation in M2 and M4)
* Date: 2026-10-22

## Context
The parity addendum requires HTTP/2 as first-class (pseudo-headers, streams,
multiplexing, metadata) and a single sending engine shared by Repeater,
Fuzzer, Crawler, Scanner, Discovery, plugins and the AI agent, with proxies,
SOCKS, pooling, redirects, cancellation, timing and resource pools.

## Options
1. Keep HTTP/1.1 text as the universal representation and render HTTP/2
   into it — loses pseudo-header order, stream data and protocol semantics.
2. Separate models per protocol with conversion at every tool boundary.
3. **One message model with a protocol tag**: HTTP/1.x keeps exact raw
   bytes; HTTP/2 keeps the ordered header list (incl. pseudo-headers), body
   frames summary, stream id and settings; a lossless *display* projection
   to HTTP/1.1-like text is derived, never stored as truth.

For sending:
A. Each tool brings its own client.
B. **One Request Engine crate** used by every sender, with a
   `SendContext` (source, task, resource pool, scope check, cancellation).

## Decision
3 + B. The current proxy upstream client becomes the Request Engine's
HTTP/1.1 path in M4; HTTP/2 uses `h2` behind the same interface (ADR-0003).

## Consequences
* ➕ No duplicated HTTP stacks; upstream/SOCKS/timeouts are configured once.
* ➕ Scope and resource pools are enforced at one choke point.
* ➖ The engine is a critical dependency; it gets its own integration suite
  (lab server + scripted edge cases) before tools migrate.
