# ADR-0024: The sequencer

* Status: Accepted
* Date: 2026-11-11

## Context
Milestone 12 adds the sequencer (spec §19): a statistical tool for the
tokens of authorized applications — length, distribution, estimated
entropy, frequency, repetition and basic patterns — with results explained
visually and without an absolute "secure / insecure" conclusion based on a
single metric.

## Decisions
1. **Pure parts in `chosproxy-sequencer`.** `TokenLocation` reads a token
   from a response: a cookie it sets, a header, or the first group of a
   regular expression over the head and decoded body. `suggest` lists the
   cookies a sample response sets, token-like headers and hidden form
   fields, so a capture can start from a stored request in one step.
2. **The analysis is a set of measurements, each explained.** `analyze`
   reports samples and repeats, the length distribution, the character set
   and the encoding it fits (decimal, hex, Base64 — only when its symbols or
   mixed case appear — Base64URL or text, with the bits one character can
   carry), Shannon entropy per character position (summed into an estimated
   entropy, an upper estimate that ignores dependencies between characters,
   with the limit the sample size imposes), bit balance for hex and Base64
   tokens with the 99% band, a monobit test, a chi-square test of character
   frequency (Wilson–Hilferty p-value), fixed prefixes and suffixes, and
   tokens that grow as numbers. Each becomes an observation (good, look
   closer, concern) with a sentence on what it means; the UI shows them
   together with a caveat that no measurement certifies a generator.
3. **A capture is a task** (kind `sequencer`, ADR-0022): the request is
   sent unchanged — no cookies from the jar, so the server issues a new
   token each time — with the usual pause, resume, stop, concurrency and
   rate limits, until the wanted number of tokens is collected. It fails
   with an explanation when half the responses carry no token. Tokens are
   stored in order (`sequencer_tokens`, schema v13); each request is an
   exchange with the new source `sequencer` (Logger, search), not passively
   analyzed (identical responses would only add noise).
4. **Charts follow the data-visualization method.** Single-series column
   charts (per-position entropy, bit balance with the expected band, token
   lengths) with thin top-rounded bars and a 2px gap, a second color only
   for flagged bars with its legend, a hover readout, a table view, and
   chart colors (`--chart-1`, `--chart-flag`) validated per theme with the
   palette checks (lightness band, chroma, colour-vision separation,
   contrast).
5. **Lab endpoints.** `/token` issues a random 128-bit token and
   `/token/sequential` a counter, to see both ends of the scale.

## Consequences
* ➕ Session and anti-CSRF tokens can be checked from a request in History
  or from tokens collected elsewhere (pasted).
* ➖ No FIPS battery beyond the monobit test, no correlation or spectral
  tests, no time-stamp decoding; these can be added as further observations.
