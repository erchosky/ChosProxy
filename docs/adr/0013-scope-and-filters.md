# ADR-0013: Scope engine, shared traffic filter and Target storage

* Status: Accepted
* Date: 2026-10-27

## Context
Milestone 5 introduces the scope (ARCHITECTURE §11): the history, the site
map, search and Intercept already need it, and every automated tool will
depend on it later. The same filters (scope, URL text/regex, source,
annotations, one site map node) apply to history, the site map and search.

## Decisions
1. **Scope is a pure, compiled function in `core`.** `ScopeConfig`
   (`scope.toml`, a separate configuration section) holds include and
   exclude rules: protocol, host (exact, optionally with subdomains, or an
   anchored case-insensitive regex), port, and path (prefix, or a regex
   searched in the path; the query string is ignored). `Scope::compile`
   validates everything, including disabled rules, so enabling a rule never
   fails later. In scope = an enabled include rule matches and no exclude
   rule matches; **no include rules = nothing in scope** (safe default for
   future automated tools). Regexes are size-limited (1 MiB compiled).
2. **Scope is evaluated, not stored.** Exchanges do not carry an `in_scope`
   column: the project database registers a SQLite function
   `chos_in_scope(scheme, host, port, path)` on its read connections, backed
   by the current compiled scope. Changing the scope therefore re-classifies
   all history immediately, with no rewrite.
3. **One filter model (`HistoryQuery`)** is compiled to SQL and used by
   history pages, the site map and search (FilterEngine, first version).
   URL regexes go through a `regexp()` SQL function with a per-statement
   compiled-regex cache; invalid expressions are reported, not treated as
   "no match".
4. **The site map is an aggregation query**, not a separate table: origin +
   path without query, grouped, capped at 20 000 endpoints. The tree is
   built in the UI. Only user marks (interesting / reviewed / ignore /
   favorite) are stored (`site_marks`, schema v3).
5. **Annotations** (note + one of eight highlight colours) are columns on
   `exchanges`, written through the single writer thread.
6. **Search runs in the engine**, newest first, over at most 100 000
   exchanges and 1 000 hits; bodies are searched after removing chunked
   framing and gzip/deflate/brotli (same decoder as the viewer, bounded
   output). A new search supersedes the running one (generation counter).
7. **Intercept "only in scope"** is a rule flag; the hold queue keeps the
   current scope next to its rules.

## Consequences
* Scope checks in SQL run Rust per row; acceptable for local projects
  (hundreds of thousands of rows); revisit with a cached column if needed.
* The scope is global (user configuration) until project settings layers
  (M8) allow a per-project override.
* Automated tools (M10+) must call `Scope::contains` before sending; the
  Request Engine will enforce it (ADR-0009).
