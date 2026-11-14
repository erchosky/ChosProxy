# Performance

Measured on an Apple Silicon laptop (release build) with
`cargo test --release -p chosproxy-app --test perf -- --ignored --nocapture`.

## Large project (100 000 exchanges, 167 MB)

| Operation | Time |
|---|---|
| Insert 100 000 exchanges (writer queue) | ~0.85 s |
| Open project | ~9 ms |
| History page (no filter / last page) | 1 ms / 5 ms |
| History text / regex / in-scope / site-node filters | 5–20 ms |
| Dashboard counters | ~45 ms (refreshed at most every 1.5 s) |
| Site map (all / in scope) | ~100 ms / ~20 ms |
| Global search, full scan of bodies | ~300 ms (was 1.2 s) |
| Exchange detail | <1 ms |
| Export copy (`VACUUM INTO`) | ~0.27 s |

## Proxy under load

64 keep-alive clients × 100 requests through a listener to the local lab:
~30 000 requests/s, p50 2.2 ms, p99 3 ms, every exchange stored, none
dropped.

## Analysis throughput (M16)

`cargo test --release -p chosproxy-app --test perf analysis_throughput -- --ignored --nocapture`, one core:

| Work | Time |
|---|---|
| Passive checks on a 39 KB HTML exchange | ~150 µs (≈6 800 exchanges/s) |
| Crawler link discovery, page with 1 000 links | ~1.1 ms |
| Sequencer analysis at its cap (100 000 tokens × 32 hex) | ~150 ms |

The M16 rerun of the large-project and proxy measurements matched the
figures above (proxy: ~25 000 requests/s, p99 3.6 ms on this run).

## Quality pass after M31b (2026-11-14)

Measured before and after on the same machine, each test alone
(`--test-threads=1`); the first run of the day was taken while other
builds ran and is not used.

| Measure | Before | After |
|---|---|---|
| Startup script of the desktop app (`vite build`, `index-*.js`) | 866 kB (287 kB gzip) | 339 kB (104 kB gzip) |
| History sorted by response length, page at offset 50 000 | 89.6 ms | 0.8 ms |
| Insert 100 000 exchanges | 1 006 ms | 880 ms (same within noise) |
| History sorted by duration (no index) | — | 87 ms (left as it is, see below) |
| Open project / first history page | 9.9 ms / 0.8 ms | unchanged within noise |
| Site map (all / in scope) | 90 ms / 20 ms | 83 ms / 19 ms (unchanged) |
| Global search (hit / rare text) | 235 ms / 297 ms | 202 ms / 256 ms (unchanged) |
| Proxy under load | 23 500 req/s, p99 4.1 ms | 23 700 req/s, p99 3.6 ms (unchanged) |
| Passive checks, 39 KB page | 144 µs | 136 µs (unchanged) |

What changed:

* **The code editor was loaded with the shell.** The engagement dialogs
  (CSRF proof of concept, clickjacking, target analyzer) import the code
  editor and were mounted by the shell, so the startup script had grown
  from the 265 kB of the M9 pass to 866 kB. They are now loaded when one
  is opened. `node scripts/check-bundle.mjs` (after `npx vite build`)
  fails if the startup script passes 400 kB.
* **An index on `exchanges(response_len)`** (schema v27): sorting the
  history by length read and sorted every row.

What was measured and left alone:

* An index on `duration_ms` made sorting by duration as fast (0.8 ms) but
  made inserting 100 000 exchanges 40 % slower (1 405 ms, three runs), so
  it was not kept: capture speed matters more than that sort.
* The other history filters, the site map, the dashboard, global search,
  the proxy and the analysis figures are where earlier passes left them.
* React re-renders were looked at in the code (store selectors return
  stored values or numbers; the history table and the hex view are
  virtualized; history events reach the UI at most every 250 ms) but not
  measured in the running app, so nothing was changed there.

## What was changed (M16)

* Fuzzer result rows are memoized by id: results reload about once a second
  during an attack and a page holds up to 2 000 rows, which no longer
  re-render unless their selection changes.

## What was changed (M9 performance pass)

* Global search reads exchanges in batches inside one read transaction with
  cached statements, and matches on up to 8 threads. Reading exchange by
  exchange from several threads was slower: SQLite's WAL locks serialise
  connections of one process.
* The UI refreshes only the traffic list on screen, at most every 300 ms,
  and dashboard counters at most every 1.5 s; lists keep their rows while
  reloading and drop pages far from the viewport.
* The WebSockets view and the event log used a debounce that never fired
  under continuous traffic; both use a throttle now.
* Views are loaded on demand: the startup script went from 865 kB to 265 kB
  and the code editor loads with the first view that needs it.
