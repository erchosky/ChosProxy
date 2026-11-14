# ADR-0037: API and GraphQL workspace

* Status: Accepted
* Date: 2026-11-12

## Context
M27 covers API scanning (SCANNER-014) and GraphQL (SCANNER-015), plus
three items deferred to it:
* editing in the Pretty view (EDITOR-001);
* multipart and XML editing in the Inspector (EDITOR-003);
* per-node parameters and requests in the site map (TARGET-003).

M27 delivered API example sends and passive analysis. Active audit of
these endpoints belongs to M13, selected next on 2026-11-14 and pending.

## Decisions
1. **A parser crate, `chosproxy-apispec`.** It reads OpenAPI 3.x,
   Swagger 2.0, Postman v2 and WSDL 1.1 into endpoints. Each endpoint has
   a method, a URL, parameters, headers and an example body.
   * **YAML** goes through `yaml-rust2`, which is safe Rust; it is turned
     into JSON values.
   * **WSDL** uses `quick-xml`, already in the lockfile, with namespaces
     resolved; this distinguishes SOAP 1.1 from 1.2.
   * **`$ref`s** are resolved only when local. Remote references and
     imports are never fetched.
   * **Bounds:** 10 MB, 5000 endpoints, a depth limit, and cycles stop.
   * **Examples** come from `example`/`default`/`enum` values, or are made
     up from the schema type and format.
   * **Security schemes** become header or query placeholders, such as
     `API_KEY` or `Bearer TOKEN`, which the tester replaces.
2. **Definitions are project data** (schema v23, `api_definitions`). The
   endpoints are stored as JSON. Importing reads a file and nothing else.
3. **Sending is explicit and bounded** (task `apiSend`):
   * only hosts the tester ticks; this is stricter than the crawler,
     because a shared collection can name third-party hosts;
   * when a scope exists, only in-scope URLs;
   * only read-only endpoints unless the tester asks otherwise.
     Read-only means GET, HEAD and OPTIONS, or an endpoint flagged
     read-only (GraphQL queries);
   * a rate limit, plus the crawler's resource pool and session rules;
   * source `scanner` in the history. The background passive analysis
     now includes that source.
   * Only the definition's example requests are sent.
4. **GraphQL builds on the same workspace** (schema v24 keeps the schema).
   * The tester picks a GraphQL request in the history. It is sent once
     more with the standard introspection query, like a Repeater send:
     scope-checked and stored in the history.
   * The schema becomes a definition whose endpoints are its queries
     (read-only) and mutations, each with a generated operation and
     example variables. Endpoints keep the original request's headers, so
     sessions carry over.
   * A schema browser and an operation builder open operations in
     Repeater or the Fuzzer.
   * A saved introspection answer can be imported instead when
     introspection is disabled.
   * The message viewer shows GraphQL operations in their own tab.
5. **Structured editing stays byte-exact.**
   * Multipart values and XML text and attributes carry their ranges. An
     edit replaces only that value (XML-escaped), and Content-Length
     follows. File parts are read-only.
   * Repeater's "Format body" and "Compact" make the body editable in its
     pretty form, without a separate lossy pretty editor.
6. **Site map nodes** list the parameter names found by the M25
   insertion-point parser in their latest 200 requests, and show their
   latest 50 requests inline.

## Consequences
* Testers can work through an API from its definition with the usual
  tools, and passive analysis and scans see the traffic.
* GraphQL endpoints get a schema-aware workflow.
* Deferred, explicitly:
  * active audit of API endpoints → M13 (selected next; implementation pending);
  * GraphQL-specific checks (introspection enabled, batching, deep
    queries) → M31;
  * pretty editing in the Intercept editor → M31;
  * fetching remote `$ref`s and imports: not planned. Definitions are
    self-contained, or the tester provides the parts.
