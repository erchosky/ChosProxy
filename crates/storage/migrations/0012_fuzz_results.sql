-- Schema v12: fuzzer results. Each attack is a task (kind "fuzz"); each
-- request is an exchange (source "fuzzer") plus one row here.

CREATE TABLE fuzz_results (
    id          INTEGER PRIMARY KEY,
    task_id     INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL,             -- 0 = baseline (original values)
    payloads    TEXT    NOT NULL,             -- JSON array of strings
    position    INTEGER,                      -- sniper: the position changed
    exchange_id INTEGER,
    status      INTEGER,
    length      INTEGER,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    error       TEXT,
    matched     INTEGER NOT NULL DEFAULT 0,   -- any match term found
    matches     TEXT    NOT NULL DEFAULT '[]',
    extracts    TEXT    NOT NULL DEFAULT '[]'
) STRICT;

CREATE INDEX idx_fuzz_results ON fuzz_results(task_id, idx);
