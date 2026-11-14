-- Schema v13: tokens collected by sequencer captures (task kind
-- "sequencer"), in capture order.

CREATE TABLE sequencer_tokens (
    id      INTEGER PRIMARY KEY,
    task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    idx     INTEGER NOT NULL,
    token   TEXT    NOT NULL
) STRICT;

CREATE INDEX idx_sequencer_tokens ON sequencer_tokens(task_id, idx);
