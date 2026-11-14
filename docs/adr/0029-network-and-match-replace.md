# ADR-0029: Outgoing network settings, invisible listeners and Match & Replace

* Status: Accepted
* Date: 2026-11-11

## Context
Milestone 21 brings the "advanced proxy and network" rows of the parity
matrix:
* upstream HTTP and SOCKS proxies chosen per destination;
* host name overrides and configurable timeouts;
* invisible proxying for clients that do not know they use a proxy;
* Match & Replace rules.

HTTP/2 end to end is large enough to be its own milestone (M21b). Platform
authentication to destination hosts needs the credential keyring of M24.

## Decisions
1. **One place for outgoing connections.**
   * Every connection to an origin goes through `Upstream::connect` (or
     `Upstream::tunnel` for opaque CONNECT tunnels) in the Request Engine.
   * The engine reads the settings from `network.toml` (`NetworkConfig`).
   * The settings are process-wide (`set_network` / `network()`), so the
     proxy, Repeater, the Fuzzer, the crawler and every later sender follow
     them without each one passing them around.
2. **Upstream rules.**
   * Rules are ordered; the first enabled rule whose host pattern matches
     wins (`*`, `example.com`, `*.example.com`). A `Direct` rule exempts
     hosts from a later catch-all rule.
   * Through an HTTP proxy:
     * TLS and opaque tunnels use `CONNECT`;
     * plain HTTP is sent in absolute form, with the request line rewritten
       at write time only (`Upstream::outgoing`). The history keeps the
       request exactly as the client sent it.
   * Through SOCKS5 (RFC 1928/1929): no authentication or user/password.
     Names are resolved by the proxy or locally.
   * Proxy credentials are Basic only. They are stored in `network.toml`,
     which is written with private permissions like every configuration
     file. They are never logged or put in the audit log, and the settings
     export does not include this file. A keyring replaces this in M24.
3. **Host name overrides.**
   * Exact or `*.domain` names map to fixed addresses, used before DNS.
   * Overrides also apply to the target sent through an upstream proxy, so
     a lab name works the same with and without a proxy.
4. **Timeouts.**
   * Connect, response and idle timeouts are configurable (1–3600 s). They
     are used by the proxy relay and as the Request Engine defaults
     (Repeater).
   * Automated tools keep their per-task timeout.
5. **Invisible listeners.** A listener option:
   * Origin-form requests go to their Host header (port 80 by default).
   * A connection that starts with a TLS ClientHello is read up to the end
     of the record. Its server name (SNI) picks the destination on a
     configurable port (443 by default). The bytes are replayed unchanged.
     The connection is then intercepted or passed through exactly as after
     `CONNECT`.
   * A TLS client without a server name is refused and reported.
   * Proxy credentials cannot be combined with invisible mode (invisible
     clients cannot send them).
   * **Loop guard:** a request whose upstream connection lands on one of
     the proxy's own listeners is refused with 508. The error points to
     host name overrides; with a hosts-file redirect, the proxy would
     otherwise connect to itself.
6. **Match & Replace in the proxy.**
   * Rules live in `proxy.toml` and are compiled once when saved. Regular
     expressions are limited to 1 MiB compiled size, and invalid rules are
     refused on save.
   * Targets: request line, request headers, request body, status line,
     response headers, response body.
   * Header rules work line by line: an empty match adds a header, and an
     empty result removes the line.
   * Body rules buffer the message (up to the Intercept hold limit). They
     remove chunked framing and content coding first and then set
     Content-Length; larger messages pass unchanged with a note.
   * Rules run before Intercept, so the user sees and can further edit the
     result.
   * The history keeps the original message and marks the exchange as
     edited, with a note naming the rules that changed it.
7. **Repeater redirect policy saved with the tab** (schema v20). This
   closes the M20b deferral.

## Consequences
* A single change in the Request Engine reaches every tool, including
  future ones, and is covered by one set of tests (unit tests for SOCKS5,
  CONNECT, absolute form, SNI parsing and rules; end-to-end tests in
  `crates/proxy/tests/network.rs` and `crates/app/tests/network.rs`).
* Process-wide settings mean tests that change them run in their own test
  binary and take turns.
* Deferred, explicitly:
  * HTTP/2 end to end, the single-packet attack, per-listener TLS options,
    a fixed "redirect to host" for invisible listeners, port/regex
    passthrough rules, importing a custom CA, and OAST SMTP and ACME
    certificates → M21b;
  * Match & Replace for WebSocket messages and scripted rules → M22;
  * platform authentication and NTLM/Digest proxy authentication → M24;
  * regular-expression Intercept rules → M22 (with the filter condition
    builder).
