# ADR-0034: Keyring, platform authentication, session rules and browser crawling

* Status: Accepted
* Date: 2026-11-12

## Context
M24 covers the "Sessions & authentication" rows of the parity matrix:
* credential storage;
* platform authentication;
* session handling rules, login macros and token extraction;
* browser-powered crawling.

These capabilities must work for every tool that sends requests, and they
must not leak secrets.

## Decisions
1. **Keyring.**
   * Credentials (id, name, user name, secret) are encrypted with
     AES-256-GCM in `credentials.enc`, with a fresh nonce on every write.
     The random key is in `credentials.key`. Both files are written with
     private permissions in the data directory.
   * Secrets are write-only from the UI and never logged or exported.
     Tampering makes decryption fail.
   * OS keychain integration is deferred.
2. **Platform authentication.**
   * Rules (host pattern, Basic or Digest, credential id) live in
     `network.toml`. The engine resolves them against the keyring and hands
     them to the Request Engine, process-wide like the network settings.
   * `send()` answers a 401 challenge once, trying every rule for the host.
     Digest follows RFC 7616: MD5 or SHA-256, `qop=auth` or none, opaque
     echoed. It is checked against the RFC's example vectors.
   * The history stores the request actually sent, with a note.
   * Proxy traffic is left alone: the browser shows its own prompts.
3. **Session rules** are kept per project (project setting
   `session_rules`).
   * A rule matches by host and tool (Repeater, Fuzzer, Sequencer, crawler,
     plugins). Every tool send goes through `session_send`.
   * It adds jar cookies and sets headers with `{{var}}` and
     `{{cred:id.username|secret}}` placeholders.
   * When a header needs a variable that is not known yet, or a response
     looks expired (status list or regex), the rule runs its login macro and
     sends again once:
     * the macro replays stored requests in order, with placeholders filled
       and jar cookies;
     * it stores their cookies and extracts variables by regex;
     * it records each macro request in the history.
   * One macro runs at a time.
4. **Browser crawling.**
   * The lab browser is launched with a loopback DevTools endpoint, without
     the DOM Inspector instrumentation, and driven by a small CDP driver.
   * For each page the driver navigates, waits for `readyState ==
     complete` plus a short settle, and reads absolute links, frames and
     form actions from the rendered DOM.
   * The crawl is breadth-first, in scope (or the start origin), and stops
     at a page limit. It runs as a task.
   * Requests go through the proxy and appear in the history.
   * Recorded logins are covered by logging in in the lab browser first or
     by a session rule's macro.

## Consequences
* Every automated sender shares one authentication and session layer.
* Deferred, explicitly:
  * NTLM and Kerberos;
  * an OS keychain;
  * a master password for the keyring;
  * the crawl graph view (TARGET-007).

  All of these go to M31.
