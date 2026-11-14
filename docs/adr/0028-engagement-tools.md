# ADR-0028: Engagement tools

* Status: Accepted
* Date: 2026-11-11

## Context
Milestone 20 brings the "engagement tools" rows of the parity matrix:
content discovery with soft-404 detection, a CSRF PoC generator,
clickjacking assessment, find references, target analysis, finding
comments and scripts, site map comparison, scheduling and "simulate manual
testing". The convenience items deferred to M20 by the 1.0 audit (editor
menus, JSON field editing, saved filters, custom key bindings…) move to a
separate milestone, M20b.

## Decisions
1. **Content discovery is a task like the crawler.**
   * It sends GET requests only, through the limiter, with the scope
     checked for every URL (or the base origin while no scope exists).
   * It is refused when the base is out of scope.
   * Names come from a built-in list of common folder and file names (no
     credentials, no exploit strings), the site map and the tester.
   * Only hits are stored in the history (source `discovery`), so thousands
     of 404s do not drown real traffic.
2. **Soft 404 by fingerprints.**
   * Each folder is first probed with random names to learn its "not found"
     answers for folders and for files.
   * Responses are compared by status, redirect target (with the name
     masked) and the shape of the body. The shape is a hash after removing
     the echoed path, the name, digits and spacing.
   * A length tolerance applies only to large pages.
   * 404, 410 and 5xx are always misses. 401 and 403 are hits (protected
     content exists). `/name` redirecting to `/name/` is a folder.
3. **CSRF PoC and FrameLab produce text for review.**
   * The HTML is generated in the UI from the stored exchange, with strict
     escaping, and copied; ChosProxy never serves or executes it.
   * The CSRF PoC states what it cannot reproduce: methods other than
     GET/POST, Content-Type checks, SameSite cookies. JSON bodies use the
     text/plain technique and say so.
   * FrameLab's verdict follows browsers: CSP `frame-ancestors` wins over
     X-Frame-Options, and `ALLOW-FROM` is ignored.
4. **Analysis only reads stored traffic.**
   * The target analyzer and "Find references" send nothing.
   * The analyzer reads at most 5000 recent exchanges per site and caps its
     lists.
5. **Schedules run inside ChosProxy, not as a service.**
   * They are stored in the project and checked every 15 seconds while it is
     open.
   * Each run goes through the normal start path (settings validated when
     scheduling; scope and limits applied when due).
   * The shortest repeat interval is 5 minutes. One-off schedules turn
     themselves off, and failures are recorded and announced.
6. **"Simulate manual testing traffic" is not implemented (➖).** Its purpose
   is to generate traffic that looks like manual testing. It gives no
   testing value and would only serve to misrepresent work.

## Consequences
* Discovery on a slow target with recursion can take long; the request cap
  (default 2000), rate and concurrency limits bound it.
* Fingerprints can still mistake a very dynamic soft-404 page for content;
  each hit links to its stored response for checking.
