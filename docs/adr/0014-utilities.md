# ADR-0014: Utilities — Decoder, Comparer, Organizer and the Render view

* Status: Accepted
* Date: 2026-10-27

## Context
Milestone 6 adds the manual utilities. They handle untrusted bytes (decoded
payloads, compressed bodies, HTML) and must stay local, bounded and safe.

## Decisions
1. **Decoder in Rust (`crates/decoder`)**, pure functions over bytes: URL,
   Base64 (standard/URL-safe), hex, HTML entities, Unicode escapes,
   gzip/zlib/deflate, pretty JSON (key order kept), JWT inspection (never
   "verified"), MD5/SHA-1/SHA-256/SHA-512. Chains stop at the first failing
   step. Inputs ≤ 16 MiB; decompression output ≤ 32 MiB (bomb guard). Bytes
   cross IPC as Base64 so binary values survive.
2. **Smart decode** is a conservative heuristic: JWT, gzip/zlib magic, then
   percent escapes, entities, Unicode escapes, hex and Base64 only when the
   result is plausible text (or compressed data), finishing with pretty
   JSON; at most 10 steps. Plain words and numbers are left alone. It only
   proposes a chain; the user sees and edits every step.
3. **Comparer in the UI (TypeScript)**: the texts are already there, and the
   structured HTTP diff reuses the Inspector's parsers. Myers' O((N+M)·D)
   diff over lines, words or UTF-8 bytes with limits (200 000 tokens,
   20 000 edits; beyond that a coarse "replaced" result). Items live for the
   session (not persisted).
4. **Organizer in the project database** (schema v4): items copy their
   request/response blobs so they outlive history clean-ups; status, user
   severity, tags, notes. Writes go through a new generic writer command
   that runs in a savepoint and replies only after commit (a failed write
   leaves nothing behind).
5. **Render view** (ADR-0007 implemented): `<iframe sandbox="">` with no
   `allow-*` token, fed through `srcdoc`, with an injected CSP
   (`default-src 'none'`, inline styles and `data:` images only) on top of
   the app's own CSP, `no-referrer`, documents ≤ 2 MB. It is a static
   preview: no scripts, forms, popups, navigation or network loads.

## Consequences
* Rendered pages look incomplete (no remote assets) — accepted.
* Comparer items are lost when the app closes; persistence can come with
  the project work (M8) if needed.
