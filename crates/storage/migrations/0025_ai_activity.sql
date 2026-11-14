-- Schema v25: the AI activity log (M30). One row per thing ChosAI or
-- ChosAgent did: a model call, a tool call (with its permission and the
-- tester's decision), a message from the tester, the final answer. Rows of
-- an agent task go when the task is deleted; one-shot ChosAI rows have no
-- task.

CREATE TABLE ai_activity (
    id INTEGER PRIMARY KEY,
    task_id INTEGER REFERENCES tasks(id) ON DELETE CASCADE,
    at INTEGER NOT NULL,
    action TEXT NOT NULL,
    tool TEXT NOT NULL DEFAULT '',
    target TEXT NOT NULL DEFAULT '',
    permission TEXT NOT NULL DEFAULT '',
    decision TEXT NOT NULL DEFAULT '',
    result TEXT NOT NULL DEFAULT '',
    detail TEXT NOT NULL DEFAULT ''
);

CREATE INDEX ai_activity_task ON ai_activity (task_id, id);
