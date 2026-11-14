-- Schema v21: suppression rules. Automatic sources do not create findings
-- of `kind` on a matching host and path ('' = any).

CREATE TABLE finding_suppressions (
    id          INTEGER PRIMARY KEY,
    created_at  INTEGER NOT NULL,
    kind        TEXT    NOT NULL,
    host        TEXT    NOT NULL DEFAULT '',
    path_prefix TEXT    NOT NULL DEFAULT '',
    reason      TEXT    NOT NULL DEFAULT ''
);

CREATE INDEX finding_suppressions_kind ON finding_suppressions (kind);
