-- Schema v22: a structured summary per task (JSON), e.g. a scan's phases
-- and counters for its dashboard (M26). Empty for tasks without one.

ALTER TABLE tasks ADD COLUMN summary TEXT NOT NULL DEFAULT '';
