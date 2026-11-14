-- Schema v16: content discovery hits (task kind "discovery").

CREATE TABLE discovery_results (
    id          INTEGER PRIMARY KEY,
    task_id     INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    found_at    INTEGER NOT NULL,
    url         TEXT    NOT NULL,
    kind        TEXT    NOT NULL,
    status      INTEGER NOT NULL,
    length      INTEGER NOT NULL,
    exchange_id INTEGER
) STRICT;

CREATE INDEX idx_discovery_results_task ON discovery_results(task_id, id);
