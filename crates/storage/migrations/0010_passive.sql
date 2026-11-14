-- Schema v10: passive analysis. The analyzer reads the history in id order
-- and remembers the last exchange it looked at; its findings use the
-- existing findings tables (source "passive", merged by dedup_key).

CREATE TABLE passive_state (
    id               INTEGER PRIMARY KEY CHECK (id = 1),
    last_exchange_id INTEGER NOT NULL DEFAULT 0,
    updated_at       INTEGER NOT NULL DEFAULT 0
) STRICT;

INSERT INTO passive_state (id, last_exchange_id, updated_at) VALUES (1, 0, 0);

CREATE INDEX idx_findings_source ON findings(source);
