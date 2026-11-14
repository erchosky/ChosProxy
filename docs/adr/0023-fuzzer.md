# ADR-0023: The fuzzer

* Status: Accepted
* Date: 2026-11-11

## Context
Milestone 11 adds the fuzzer (spec §25): a generic engine for authorized,
repetitive tests with payload positions in a request (`username=§value§`),
one or several positions, custom lists, numbers, sequences and the tester's
own dictionaries, a results table (request number, payload, status,
length, duration, error, match), rate limiting, concurrency, pause,
resume, cancel and a retry policy. It must not ship lists of real
credentials nor offer automation aimed at password spraying or credential
stuffing.

## Decisions
1. **Pure parts in `chosproxy-fuzzer`.** `Template` reads `§…§`
   positions (balanced, at most 20) and renders values into them;
   `auto_mark` marks query, cookie and URL-encoded form values. `PayloadSet`
   produces values from the tester's list, a number range (step, zero
   padding), character combinations (lengths 1–8) or a built-in list, then
   applies processing rules in order: prefix, suffix, replace, case and any
   encoding or hash of the Decoder (`chosproxy-decoder::apply`). A set holds
   at most 200,000 values; a payload at most 64 KiB.
2. **Four attack modes addressed by index.** Sniper (one set, each position
   in turn), battering ram (same payload everywhere), pitchfork (one set per
   position, in lockstep) and cluster bomb (every combination). `Plan`
   computes the count up front (saturating) and the n-th request on demand,
   so nothing is materialized and the limit (`max_requests`, at most
   200,000) is checked before anything is sent.
3. **A baseline first.** Request 0 carries the original values, so the
   results can be compared with what the application normally answers
   ("status differs from baseline" filter).
4. **An attack is a task** (kind `fuzz`, ADR-0022): the same control channel
   (pause, resume, stop), counters, log and `Limiter`; requests without a
   response are retried up to three times. Request bytes follow the
   Repeater rules (line endings restored, Content-Length recomputed unless
   turned off) and may carry cookies from the jar.
5. **Results are project data.** Every request is an exchange with source
   `fuzzer` (Logger, search, passive analysis — which is where database
   errors provoked by payloads show up) plus a row in `fuzz_results`
   (schema v12): payloads, position, status, length, time, error, one flag
   per grep match term and one value per extract pattern. The table is
   sorted and filtered in SQL (matched only, status differs from the
   baseline) and paged.
6. **Responsible defaults.** 4 requests at a time and 10 per second by
   default; the built-in lists are generic test inputs (special characters,
   boundary values, harmless probes that reveal injection points through
   errors or reflection); there are no username or password lists and no
   features to rotate credentials across accounts.

## Consequences
* ➕ Manual testing of inputs at scale, with results that stay in the
  project and feed passive analysis.
* ➕ The plan/limiter/task pieces are reusable by the active audit framework
  (M13).
* ➖ No payload processing by script, no recursive grep or response-based
  stopping rules yet; shared resource pools come with M26.
