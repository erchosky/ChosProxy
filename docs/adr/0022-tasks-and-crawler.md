# ADR-0022: Tasks and the crawler

* Status: Accepted
* Date: 2026-11-11

## Context
Milestone 10 adds the first automated tool that sends requests on its own:
the crawler (spec §13). The spec also asks for a task system (§32) shared
by crawls, audits, imports and fuzzing — states queued, running, paused,
completed, failed and cancelled, with progress, duration, requests, errors
and logs — and for automated tools to be restricted to the scope by
default (§12) and never to run dangerous actions.

## Decisions
1. **Tasks are project data.** Schema v11 adds `tasks` (kind, name,
   status, the configuration it started with as JSON, times, counters,
   short progress text, last error) and `task_log` (capped at 2000 lines per
   task). Tasks left unfinished when a project closes are recorded as
   cancelled, with a log line saying why; resuming across sessions is not
   offered.
2. **A running task is a tokio task with a control channel.** The engine
   keeps a watch channel per running task (run / pause / cancel). Pausing
   lets requests in flight finish and starts no new ones; cancelling aborts
   them. Counters are saved about once a second with a `tasksChanged`
   event, so the UI follows without polling.
3. **The first resource-pool limiter lives in the Request Engine.**
   `Limiter` bounds requests in flight and spaces their starts
   (requests per second). Each crawl has its own; shared pools across tools
   come with M26.
4. **The crawler's pure parts are a crate, `chosproxy-crawler`:**
   `CrawlConfig` (validated limits), `discover` (links, frames, scripts,
   GET and POST forms, `<base>`, meta refresh, `Location`/`Refresh`, URLs
   and paths in inline and external JavaScript, robots.txt and sitemap
   entries — tolerant patterns rather than a full HTML parser) and
   `Frontier` (breadth first, each URL once, depth limit, a cap on query
   variants per path, static files skipped, words to avoid).
5. **Safe by construction.** The crawler sends only GET requests; POST
   forms are reported in the task log, never submitted; GET forms are
   submitted with default values only when enabled; links whose path or
   query contains state-changing words (logout, delete, remove,
   checkout…) are not followed, except start URLs the tester typed. It
   stays in scope — and, while no scope is defined, on the start URLs'
   origins — and refuses to start when no start URL is allowed.
6. **Its traffic is ordinary, labelled traffic.** Requests look like a
   browser's (with cookies from the project jar when enabled, so a crawl
   can run as the logged-in user) and every exchange is stored with source
   `crawler`: it shows in the Logger, the site map and search, and goes
   through passive analysis (ADR-0021) like proxy traffic. Cookies it
   receives go to the jar.

## Consequences
* ➕ Mapping a site no longer needs clicking through it; findings appear
  while it runs.
* ➕ The task system, limiter and frontier are ready for the fuzzer (M11)
  and the active audit framework (M13).
* ➖ No JavaScript execution (SPAs that build links at runtime are only
  partly covered by the JavaScript URL patterns); browser-powered and
  authenticated crawling with recorded logins come with M24.
* ➖ Tasks do not resume after the project is reopened.
