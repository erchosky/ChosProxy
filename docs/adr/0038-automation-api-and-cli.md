# ADR-0038: Automation API and `chosproxy-cli`

* Status: Accepted
* Date: 2026-11-12

## Context
M28 covers PLATFORM-003 (REST API) and PLATFORM-004 (headless / CLI). The
engine was already independent of the UI. What was missing was a way for
scripts to drive it, and a way to run it without a window, without opening
a new door into the tester's machine.

## Decisions
1. **A separate crate, `chosproxy-api`.** It runs a hyper HTTP/1.1 server
   over `chosproxy_app::Engine`, with JSON under `/v1/`.
   * It exposes the operations the UI already has, and nothing more:
     projects, listeners, history, findings, scope, a Repeater-like send,
     crawls, scans, API definitions, tasks and reports.
   * The engine keeps every check: scope, safe defaults and limits.
2. **Off by default, loopback only.**
   * `api.toml` holds `enabled = false` and port 8790.
   * The server binds `127.0.0.1` and ignores peers that are not loopback.
   * `chosproxy-cli serve` turns it on explicitly for that process.
3. **A token on every call.**
   * The token is 32 random bytes in hex, in a private file
     (`api.token`), and can be regenerated.
   * It is sent as `Authorization: Bearer`, and compared in constant
     time.
   * Clients read it from `--token`, `CHOSPROXY_API_TOKEN` or the file.
4. **No browser access.**
   * Requests with an `Origin` header are refused, so web pages cannot
     reach the API, even with a guessed token.
   * The `Host` header must name the loopback listener, which defeats DNS
     rebinding.
   * Bodies are capped at 10 MB.
5. **Audited.** Calls that change scope, listeners, the project or the
   token, or that start crawls and scans, are written to the project
   audit log as `api.*`.
6. **`chosproxy-cli` is one binary.** `serve` runs headless: engine,
   project, the listeners set to start with a project, and the API,
   until Ctrl-C. The other commands are thin clients of the API, using a
   plain HTTP/1.1 call to 127.0.0.1 with no extra dependencies. The Kali
   package ships it next to the desktop app.
7. **The desktop runs the same server.** It starts the server when the
   API is enabled, and restarts it when the settings or the token change.

## Consequences
* CI jobs and scripts can run crawls, scans and reports against
  authorized targets without a window.
* The attack surface is a loopback port behind a token, closed to web
  pages. Local malware with the user's rights could read the token, as it
  could read the projects.
* Deferred, explicitly:
  * the OAST items that were deferred into M28 (SMTP and ACME for
    ChosOAST, OAST providers via plugins) → M31, because they open new
    listeners or talk to outside services;
  * an OpenAPI description of this API, and streaming of engine events
    over the API → M31.
