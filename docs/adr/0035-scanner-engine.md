# ADR-0035: Scanner engine — insertion points, fingerprints, scans and suppressions

* Status: Accepted
* Date: 2026-11-12

## Context
M25 covers the scanner-engine rows of the parity matrix:
* insertion points;
* deduplication;
* scan phases, configurations, launcher and debugging views;
* JavaScript analysis;
* false-positive handling;
* scheduled scans.

M25 delivered scanner infrastructure and passive analysis. Active audit
belongs to M13, which is not implemented and was selected next by the
user on 2026-11-14. This ADR describes the passive M25 implementation.

## Decisions
1. **Scope: infrastructure and passive analysis only.**
   * A scan sends only what the crawler sends: GET requests from links,
     in scope (or on the start origin), rate-limited, avoiding words like
     "logout" or "delete".
   * Every audit phase reads stored traffic.
   * Active checks, and "checks that send requests", stay with M13.
2. **Insertion points** (`chosproxy_scanner::insertion`).
   * A parser works on the request text the editors use. It lists path
     segments, query, form, cookie, chosen headers, JSON (any depth),
     multipart (fields and file names) and XML (text and attributes)
     values.
   * Each value comes with its byte range and the encoding its context
     needs (URL, path, cookie, JSON string or scalar, XML, header, quoted,
     raw).
   * `replace` writes a value back without breaking the request; `mark`
     turns points into Fuzzer positions.
   * The Fuzzer's old ad-hoc auto-mark was replaced by it.
3. **Fingerprints** (`chosproxy_scanner::fingerprint`).
   * The fingerprint is the method, the origin, the path with
     variable-looking segments folded (numbers, UUIDs, long hex, long
     mixed tokens), and the sorted parameter names, including body fields
     (array indexes folded).
   * The crawler limits URLs per pattern (`maxSimilar`, default 20; seeds
     are always fetched).
   * A scan audits each request fingerprint once.
4. **Scans** (`engine/scan_ops.rs`).
   * A scan is one task with phases: crawl (reusing the crawl task code),
     then audit.
   * The audit runs the passive checks, check scripts and JavaScript
     analysis on each new fingerprint. It reads the crawled items, or,
     without a crawl, the stored traffic under the start URLs.
   * Findings are merged through `findings_record_auto`.
   * The task log is the scan's event log: phases, and for each item
     "Audited … first item of pattern …" or "Skipped … same pattern as
     #id". The first 300 decisions are written; the rest are counted.
5. **Configuration library.**
   * Four built-ins are read-only, and none is unthrottled.
   * The tester's configurations go in `scans.toml` (at most 100), with
     JSON import/export (format `chosproxy-scan-configs`).
   * Imports get unique names and never start anything.
   * Scans can be scheduled like crawls (schedule kind `scan`).
6. **JavaScript analysis** (`chosproxy_scanner::javascript`) runs on
   successful script responses only. It reports:
   * endpoints referenced in string literals;
   * source maps (comment or header);
   * known secret formats and long values assigned to secret-like names,
     always masked in evidence;
   * DOM sinks (presence only; tracing is the DOM Inspector's job).
7. **Suppression rules** (schema v21).
   * A rule is a finding type, a host (`*.example` for subdomains, empty
     for any) and a path prefix, with a reason.
   * The rules are checked inside the transaction that records automatic
     findings (passive analysis, scans, plugins). Manual findings are
     never affected.
   * Adding a rule can mark the matching open findings as false
     positives. Rules are listed, deletable and audited.

## Consequences
* The Fuzzer finds JSON, multipart and XML positions.
* Crawls stop wasting requests on id-only variations.
* Scans give a reproducible, explained audit of a site, without any
  attack traffic.
* Deferred, explicitly:
  * the active audit and checks that send requests (M13, selected next; implementation pending);
  * runtime instrumentation (INSTR-001 → M31, needs a design);
  * JavaScript analysis in the background analyzer, a scan dashboard,
    and per-check selection in the UI → M26;
  * Pretty-view and multipart/XML Inspector editing → M27;
  * raw and streaming HTTP/2 → M31;
  * OAST SMTP/ACME and OAST providers via plugins → M28.
