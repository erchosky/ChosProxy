# ADR-0018: Reporting

* Status: Accepted
* Date: 2026-10-30

## Context
Milestone 15 turns documented findings (ADR-0017) into a deliverable
report. Reports contain captured traffic and tester-written text, both
untrusted, and are shared outside the tool, so they must be safe to open
and must not leak credentials by accident.

## Decisions
1. **One model, three renderers.** A new `chosproxy-report` crate builds a
   `Report` (meta, scope lines, target hosts, findings sorted by severity
   then title) and renders it as HTML, Markdown or JSON
   (`chosproxy-report-v1`). The crate depends only on `chosproxy-core`; the
   engine maps stored findings into it.
2. **HTML is self-contained and inert.** Every value is escaped; inline CSS
   only; no scripts, links or external resources; a
   `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'`
   meta tag. The in-app preview shows it in an iframe with `sandbox=""`
   (ADR-0007).
3. **Markdown cannot be restructured by content.** Inline text escapes
   Markdown punctuation and `<`/`>`; leading list markers are neutralised;
   evidence goes in fenced blocks longer than any backtick run inside.
4. **Evidence is redacted by default.** Values of Authorization,
   Proxy-Authorization, Cookie, Set-Cookie, X-Api-Key, X-Auth-Token and
   CSRF token headers are replaced (cookie names and the auth scheme stay).
   Bodies are not changed; the UI says so. Messages are cut at a per-message
   limit (1 KiB–1 MiB, default 64 KiB) on a character boundary and marked
   as shortened. Evidence can be left out entirely.
5. **The tester chooses the findings.** Every finding except false
   positives is included unless the tester leaves it out; explicit choices
   are remembered while the project is open, so findings documented later
   are included without revisiting the view. The report includes the scope
   rules in effect and the hosts of the chosen findings.
6. **Exports are audited**: format, number of findings, whether evidence
   was included and unredacted, and the destination path.

## Consequences
* PDF can be produced later from the same HTML (print styles are already
  in place); templates and branding can be added as alternative renderers
  over the same model.
