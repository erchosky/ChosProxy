# ADR-0031: HTTP/2 through the proxy

* Status: Accepted
* Date: 2026-11-12

## Context
ADR-0030 gave the Request Engine HTTP/2, but browsers still had to use
HTTP/1.1 with the proxy. Some applications behave differently over HTTP/2,
and some only serve it. The relay of M1 streams HTTP/1.1 byte for byte, and
Intercept, Match & Replace and the history work on whole text messages.

## Decisions
1. **Opt-in per listener.** "HTTP/2" on a listener makes the interception
   certificate offer ALPN `h2` before `http/1.1`. When the client picks
   `h2`, the decrypted connection is served by an HTTP/2 server (the `h2`
   crate) instead of the HTTP/1.1 session.
2. **Whole messages per stream.**
   * Each stream is read completely and turned into the text form of
     ADR-0030: the request line from `:method` and `:path`, `host` from
     `:authority`, separate `cookie` fields joined with `; `.
   * Match & Replace and Intercept then work exactly as for HTTP/1.1, and
     the history stores `HTTP/2` messages.
   * Request bodies are limited to the Intercept hold limit (8 MB) and
     responses to 64 MB. At most 32 streams run at once per client
     connection, which bounds memory.
3. **To the server.**
   * One HTTP/2 connection per client connection carries every stream
     (`H2Connection`). It is replaced after a failure.
   * When the server does not agree to `h2`, each request is sent as
     HTTP/1.1 (version token changed, `Content-Length` set) and the history
     says so. Upstream ALPN now offers `h2, http/1.1` so such servers still
     complete the handshake.
   * Chunked HTTP/1.1 responses are unframed, and hop-by-hop headers are
     dropped before the response goes back over HTTP/2.
4. **Invisible listeners can redirect everything** to a fixed `host:port`.
   The server name still picks the certificate and the upstream SNI.
5. **Passthrough patterns can name a port** (`host:8443`). A pattern
   without a port matches every port. IPv6 literals are not mistaken for
   `host:port`.

## Consequences
* Browsers get HTTP/2 through the proxy with the same tools as HTTP/1.1.
* Bodies are not streamed over HTTP/2: very large downloads (over 64 MB)
  fail with a clear 502, and long-lived responses (server-sent events) wait
  until they end. Listeners keep HTTP/1.1 by default for this reason.
* WebSockets over HTTP/2 (RFC 8441) are not offered; browsers open an
  HTTP/1.1 connection for them, which is captured as before.
* Deferred, explicitly:
  * streaming HTTP/2 bodies and raw or invalid HTTP/2 → M25;
  * a certificate per listener → M31;
  * regex and scope-based passthrough rules → M22;
  * OAST SMTP and ACME certificates → M25.
