# ADR-0019: Repeater and the Request Engine send path

* Status: Accepted
* Date: 2026-11-10

## Context
Milestone 4 adds Repeater: edit a request, send it, read the response,
repeat. ADR-0009 requires one Request Engine for every sender. Repeater is
the first tool that sends requests itself, so it defines the send path that
the fuzzer, crawler and scanner will reuse. Tabs must live in the project
(spec §34) and every send must be traceable in the Logger (spec §21).

## Decisions
1. **The send path lives in `chosproxy-request`.** `send(destination,
   security, bytes, limits)` opens a connection (the same `Upstream` the
   proxy uses), writes the bytes **exactly as given** — malformed requests
   included — and reads the response with the wire-exact codec: interim 1xx
   responses skipped and counted, `HEAD`/204/304/101/`CONNECT` without a
   body, chunked and close-delimited bodies, a size cap (the capture limit)
   and response/body idle timeouts. It returns the raw response plus
   timing split into DNS, connect, TLS, server wait and download. A failure
   before the response head is an error with the time spent; a body that
   breaks off is a response marked incomplete. Each send uses a new
   connection (no pooling yet).
2. **Cancellation is task abort.** The engine runs each send as a task and
   keeps its abort handle per tab; *Cancel*, closing the tab and closing the
   project abort it. A cancelled send is not recorded. A tab sends one
   request at a time.
3. **The editor holds text, the wire gets bytes.** A tab stores the
   request as editor text (`\n` line endings) plus the head and body
   line-ending conventions of the message it came from, restored on send
   (ADR-0012). `Content-Length` is recomputed on send unless the tester
   turns that off (to test framing). Requests with compressed or chunked
   bodies are unpacked when opened in Repeater; other binary requests are
   refused with an explanation.
4. **Storage (schema v8).** `repeater_tabs` (name, order, target, request,
   line endings, options, a path for scope) and `repeater_sends` (tab,
   exchange, timing). Each send is an ordinary exchange with source
   `repeater`, so it appears in the Logger, search and evidence like any
   other traffic; closing a tab keeps those exchanges. Tabs are saved
   shortly after each edit and with every send. In-scope project exports
   drop out-of-scope tabs.
5. **TLS follows the proxy's policy.** HTTPS sends use the proxy's
   upstream TLS configuration and its "skip certificate verification" host
   list, so a lab host trusted for the proxy is trusted in Repeater too.
   Without a local CA, Repeater builds its own client configuration once.
6. **Redirects are followed on request.** "Follow redirect" builds the
   next request like a browser: the `Location` resolved against the sent
   URL, 303 (and 301/302 after POST) become GET without a body, `Host` is
   updated, and `Authorization`/`Cookie` are dropped when the origin
   changes; what changed is reported. The tester sees and sends it.
7. **Scope is not enforced for manual sends.** Repeater is a deliberate,
   manual action; scope enforcement in the Request Engine applies to
   automated tools (M10).

## Consequences
* ➕ Later senders reuse the send path, timing and storage conventions.
* ➕ Repeater traffic is first-class project data (Logger, search,
  findings evidence, reports).
* ➖ No HTTP/2, upstream proxies or resource pools yet; they plug into the
  same function (M21, M26).
* Tab groups, group sends, connection reuse and the cookie jar followed in
  ADR-0020.
