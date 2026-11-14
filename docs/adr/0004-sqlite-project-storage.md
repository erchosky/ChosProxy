# ADR-0004: SQLite per project, single writer thread, compressed blobs

* Status: Accepted
* Date: 2026-10-21

## Context
Projects must survive tens of thousands of exchanges without loading the
project in RAM or blocking the UI. A project should be one portable file.

## Options
1. **rusqlite (bundled SQLite)**, WAL, one writer thread, read pool.
2. sqlx (async SQLite) — async API but its SQLite driver still runs on a
   blocking thread internally; compile-time query checking needs a DB.
3. Embedded KV store (sled/redb) + own indexing — no SQL filtering/FTS.

## Decision
Option 1. Schema with metadata in `exchanges` and bodies in `blobs`
(zstd-compressed above 1 KiB, flagged per row). Migrations are numbered SQL
files embedded in the binary and tracked with `PRAGMA user_version`.
Keyset/offset paging; indexes on every filtered column.

## Consequences
* ➕ Single-file projects, transactional, fast filtering, FTS available later.
* ➕ Bundled SQLite: identical behaviour across distros.
* ➖ Writes funnel through one thread: batched transactions keep throughput
  high; a bounded queue applies back-pressure (captures counted as dropped
  rather than stalling traffic).
* ➖ Offset paging is O(offset); acceptable for the 10⁴–10⁵ target, revisit
  with keyset windows if profiling (M16) shows need.
