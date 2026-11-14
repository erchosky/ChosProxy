-- Schema v18: scheduled tasks (SCANNER-016). They run only while the
-- project is open in ChosProxy.

CREATE TABLE schedules (
    id             INTEGER PRIMARY KEY,
    kind           TEXT    NOT NULL,          -- crawl | discovery
    name           TEXT    NOT NULL,
    config         TEXT    NOT NULL,          -- the task's JSON settings
    next_run       INTEGER NOT NULL,          -- unix epoch ms
    repeat_minutes INTEGER NOT NULL DEFAULT 0, -- 0 = once
    enabled        INTEGER NOT NULL DEFAULT 1,
    last_run       INTEGER,
    last_task_id   INTEGER,
    last_error     TEXT
) STRICT;
