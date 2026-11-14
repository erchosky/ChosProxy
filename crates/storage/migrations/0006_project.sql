-- Schema v6: project settings layer, append-only audit log, Organizer
-- collections.

-- Per-project settings (JSON values) that override the user's settings, plus
-- project-only settings such as backups and notes.
CREATE TABLE project_settings (
    key        TEXT PRIMARY KEY,
    value      TEXT    NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

-- Security-relevant actions on this project. Rows are only ever inserted.
CREATE TABLE audit_log (
    id     INTEGER PRIMARY KEY,
    at     INTEGER NOT NULL,                 -- unix epoch ms
    action TEXT    NOT NULL,
    detail TEXT    NOT NULL
) STRICT;

CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log
BEGIN SELECT RAISE(ABORT, 'the audit log is append-only'); END;
CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log
BEGIN SELECT RAISE(ABORT, 'the audit log is append-only'); END;

ALTER TABLE organizer_items ADD COLUMN collection TEXT NOT NULL DEFAULT '';
