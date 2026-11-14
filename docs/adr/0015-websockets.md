# ADR-0015: WebSocket capture and interception

* Status: Accepted
* Date: 2026-10-28

## Context
Until M7 a `101 Switching Protocols` turned the connection into an opaque
byte splice. Milestone 7 captures WebSocket handshakes and messages, lets the
user pause/edit/drop messages, and search and export them.

## Decisions
1. **Own frame codec (`crates/websocket`)** for RFC 6455: frames are read
   with their exact wire bytes kept, so unmodified messages are forwarded
   byte for byte (masking keys included). Payloads above 64 MiB end the
   connection. Fragmented messages are joined for capture; control frames
   that arrive between fragments are forwarded immediately.
2. **Extensions are removed from the handshake.** The proxy drops
   `Sec-WebSocket-Extensions` from upgrade requests (noted on the exchange),
   so servers never negotiate compression and every payload is readable and
   editable. Frames that still carry RSV bits are relayed untouched, flagged,
   and never paused.
3. **Frame-by-frame relay** in both directions after a WebSocket `101`;
   other upgrades keep the opaque splice. Once both sides have sent a close
   frame (or one side ends, after a 5 s grace) the connection is finished.
4. **Interception per message**, opt-in per direction
   (`websocket_to_server`, `websocket_to_client`), filtered by the
   Intercept host and "only in scope" rules. Only text and binary data
   messages ≤ 8 MiB are paused; while a fragmented message is paused its
   fragments are buffered. An unedited message is forwarded as its original
   frames; an edited one as a single new frame (masked with a fresh key
   towards the server). Dropped messages are not delivered. The paused
   message is abandoned if its connection ends.
5. **Storage (schema v5)**: `ws_connections` keyed by a unique connection
   key that is also stored on the handshake exchange (`exchanges.ws_key`);
   `ws_messages` with direction, opcode, length, payload blob, original
   blob when edited, and flags (edited, dropped, truncated, extension).
   Events go through the same single writer queue as exchanges, in order.
6. **Search and export** run in Rust: message search filters by direction
   and control frames in SQL and scans payloads (≤ 100 000 messages); export
   writes one connection as JSON (`chosproxy-websocket-v1`, text payloads as
   text, binary as Base64).

## Consequences
* Applications that require `permessage-deflate` (rare) see uncompressed
  traffic; compression support for display can come later.
* Manual re-sending of WebSocket messages belongs to the Repeater (M4).
* WebSockets over HTTP/2 (RFC 8441) arrive with HTTP/2 support (M21).
