# ADR-0027: DOM Inspector

* Status: Accepted
* Date: 2026-11-11

## Context
Milestone 19 adds the DOM Invader equivalent of spec addendum §69: in the
lab browser, find where sources (URL parts, referrer, `window.name`,
cookies, `postMessage`) reach dangerous sinks, report client-side
redirects, message listeners, prototype pollution and DOM clobbering, and
show SOURCE → TRANSFORMATION → SINK with stack, file, line and value. The
spec requires dynamic instrumentation, not static string matching.

## Decisions
1. **Instrumentation through the Chrome DevTools Protocol.** The lab browser
   is launched with `--remote-debugging-port=0 --remote-debugging-address=127.0.0.1`
   only when the DOM Inspector is chosen. ChosProxy reads the port from the
   profile's `DevToolsActivePort` (the profile directory is `0700`),
   auto-attaches to every page and out-of-process iframe with
   `waitForDebuggerOnStart`, and installs the script with
   `Page.addScriptToEvaluateOnNewDocument` before letting each one run.
   The script runs before the page's own scripts and page CSP does not
   apply to it.
   Rejected alternatives:
   * Injecting through the proxy would rewrite every HTML response and
     relax CSP, changing what is being tested.
   * A browser extension cannot be loaded from the command line in Google
     Chrome since version 137.
2. **Reports through a DevTools binding, not the network.** The script
   captures the binding (`Runtime.addBinding`, a random name per session)
   and removes it from the page before any page script runs. Reports never
   appear in traffic or in the history.
3. **The script only observes.** Hooked setters and methods call the
   originals with the same arguments. `eval` is not wrapped: a wrapper would
   turn direct eval into indirect eval and change scope. The `Function`
   constructor is wrapped in a Proxy so `instanceof` and calls keep working.
4. **Flows are traced two ways.** A per-session canary that the tester puts
   into a source gives certain flows (Firm). Pieces of current source
   values (at least 6 characters, raw or URL-decoded) found in sink values
   give probable flows (Tentative), so flows appear even without the
   canary.
5. **Reports are untrusted input.** Page code runs in the same world and
   could imitate the script. Reports are parsed strictly (known kinds,
   64 KiB, every string cut), rate-limited and de-duplicated in the page,
   merged in the project, shown as text and fuzzed by the robustness
   harness.
6. **Observations become findings only with evidence.** An observation is
   linked to the stored response that delivered the page (same URL without
   the fragment). Only linked observations become findings, with source
   `dom`:
   * DOM-based XSS candidates (High/Firm with the canary);
   * URL injection or open redirection;
   * `postMessage` listeners that never mention the origin (Low/Tentative);
   * prototype pollution;
   * DOM clobbering (Info).
7. **Chromium family only for now.** Firefox has removed CDP; WebDriver
   BiDi (`script.addPreloadScript`, channels) is the route for it later.

## Consequences
* While the DOM Inspector runs, any local process could connect to the
  browser's DevTools port and control that lab browser. The port is
  loopback-only and random, exists only for that session and closes with
  the browser. This is recorded as T34.
* Sinks reached through direct `eval`, or through `location` setters (which
  cannot be hooked), are not seen. Navigations are seen through the
  Navigation API.
* Clobbering detection is a heuristic: it names document properties that
  elements shadow, not whether a script reads them.
