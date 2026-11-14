-- Schema v11: tasks (crawls today; later audits, imports, fuzzing) and
-- their logs.

CREATE TABLE tasks (
    id          INTEGER PRIMARY KEY,
    kind        TEXT    NOT NULL,             -- crawl
    name        TEXT    NOT NULL,
    status      TEXT    NOT NULL,             -- queued | running | paused | completed | failed | cancelled
    config      TEXT    NOT NULL,             -- JSON, as started
    created_at  INTEGER NOT NULL,             -- unix epoch ms
    started_at  INTEGER,
    finished_at INTEGER,
    requests    INTEGER NOT NULL DEFAULT 0,
    errors      INTEGER NOT NULL DEFAULT 0,
    discovered  INTEGER NOT NULL DEFAULT 0,
    queued      INTEGER NOT NULL DEFAULT 0,
    progress    TEXT    NOT NULL DEFAULT '',  -- short text, e.g. "depth 2"
    last_error  TEXT
) STRICT;

CREATE TABLE task_log (
    id      INTEGER PRIMARY KEY,
    task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    at      INTEGER NOT NULL,
    level   TEXT    NOT NULL,                 -- info | warning | error
    message TEXT    NOT NULL
) STRICT;

CREATE INDEX idx_task_log ON task_log(task_id, id);
