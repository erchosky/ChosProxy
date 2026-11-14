# ADR-0021: Passive analysis

* Status: Accepted
* Date: 2026-11-11

## Context
Milestone 9 asks for a passive scanner (spec §22): find weaknesses in
traffic that already happened — cookie flags, missing security headers,
mixed content, information leaks, permissive CORS, risky caching, sensitive
forms, JWT problems, interesting endpoints, software versions — without
sending or changing anything, and explain each problem. Findings and their
evidence already exist (ADR-0017) with a `dedup_key` reserved for automatic
sources.

## Decisions
1. **Checks are pure functions in a new crate, `chosproxy-scanner`.** A
   `Message` (parsed heads, response body dechunked, decompressed and
   decoded as text up to 2 MiB; binary bodies skipped) goes in, `Issue`s
   come out: title, severity, confidence, a short evidence note and a dedup
   scope. Each check has a stable id, an issue type, a one-line summary and
   an educational description and remediation. The crate has no storage or
   engine dependency, so plugins and the active scanner can reuse it.
2. **Eighteen first checks**, chosen to be precise rather than many:
   cookie Secure / HttpOnly / SameSite, HSTS, `nosniff`, framing
   (clickjacking), CSP (missing or allowing inline/eval scripts), software
   versions, permissive CORS (reflected origin, `null`), mixed content,
   passwords over HTTP, secrets in URL parameters, JWT weaknesses (`alg
   none`, no expiry, secret claims), verbose errors and database errors,
   private IP addresses, directory listings, cacheable authenticated
   responses and interesting or exposed paths. Heuristic ones say so with a
   *tentative* confidence (e.g. a database error is a possible SQL
   injection to confirm manually). Evidence notes never copy cookie values
   or tokens in full.
3. **One finding per problem, not per request.** The dedup key is
   `passive:<check>:<scope>`, where the scope is what makes it the same
   problem (the origin for missing headers, origin + cookie name, origin +
   path for page issues…). The first detection creates a finding (source
   `passive`); later ones add evidence up to five copies. Findings the
   tester marked *false positive* or *accepted risk* are left alone; a
   finding marked *fixed* is opened again, with a note, when the problem
   reappears.
4. **A background task follows the history.** The engine runs one analyzer
   task (started with the first project, following whichever is open, ended
   when the engine is dropped). It reads exchanges in id order from a
   cursor stored in the project (`passive_state`, schema v10), analyzes up
   to 100 per round off the async runtime, and records the findings and the
   new cursor in one transaction. Analysis therefore never slows the proxy,
   survives restarts and catches up with traffic captured while it was off.
   "Analyze all again" resets the cursor; merging makes it idempotent.
5. **What is analyzed is a setting** (`[passive]` in `settings.toml`):
   on by default; proxy traffic always, Repeater traffic optionally (on by
   default); only in-scope traffic by default — but while no scope is
   defined, all traffic, so a lab works out of the box; checks can be
   turned off one by one. Other tools' traffic (crawler, scanner) will be
   handled by their own tasks.
6. **The UI is told, not polled heavily.** A `findingsChanged` event
   refreshes the dashboard counts and an open Findings view (throttled);
   the Findings view shows where the analysis stands and can restart it.

## Consequences
* ➕ Findings appear while browsing, each with an explanation and evidence.
* ➕ The scanner crate is the place for more passive checks, plugin checks
  (M14) and the active audit framework (M13).
* ➖ Checks see one exchange at a time: nothing that needs several requests
  (e.g. comparing responses) — that belongs to active checks.
* ➖ Without a public-suffix list or browser context some checks are
  heuristics; their confidence says so.
