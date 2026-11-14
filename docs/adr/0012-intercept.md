# ADR-0012: Intercept — buffering, editing as text, decisions

* Status: Accepted
* Date: 2026-10-22

## Context
Milestone 3 pauses requests and responses so the user can edit, forward or
drop them. The relay otherwise streams bodies (ADR-0003); the editor is a
web text component; fidelity (priority #3) must hold for unedited messages.

## Decisions
1. **Buffer only what is paused.** A matching message is read completely
   (head + raw body framing) up to 8 MiB (and the capture limit); larger
   messages are forwarded unmodified with a note. The queue is bounded to
   500 messages and 256 MiB; beyond that messages pass through.
2. **Upstream is connected after the request decision**, so a dropped
   request never reaches the server and edits may change the request line.
3. **Text round-trip with recorded line endings** (`chosproxy_http::edit`):
   the head and body line endings (CRLF/LF) are recorded and restored, so
   unedited text yields the original bytes and edits keep the conventions
   (e.g. CRLF multipart). Invalid UTF-8 or mixed line endings are reported
   as not editable; such messages can be forwarded/dropped, viewed in hex,
   or **unpacked** (de-chunked, decompressed, `Content-Length` set).
4. **Content-Length update** is an explicit, default-on option applied to
   edited messages only; chunked messages are left alone.
5. **Drop answers with an explanatory 502** instead of closing the
   connection (browsers silently retry reset idempotent requests).
6. **Turning Intercept off forwards everything unmodified**; a client that
   disconnects while paused is removed from the queue; Intercept starts off
   in every session and project.
7. **Edits are recorded**: the main blobs hold what was sent/delivered, the
   originals are kept (schema v2) and flagged `edited`/`dropped`.

## Consequences
* ➕ Unedited intercepted traffic is byte-identical to non-intercepted.
* ➕ Memory stays bounded however many messages pile up.
* ➖ Paused requests hold the client connection open; slow decisions can hit
  client timeouts (as in any intercepting proxy).
* ➖ Responses to HEAD/304 etc. are paused with empty bodies, as received.
