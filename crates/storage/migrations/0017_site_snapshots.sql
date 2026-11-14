-- Schema v17: named site map snapshots, for comparison (TARGET-005).

CREATE TABLE site_snapshots (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL,
    created_at INTEGER NOT NULL,
    endpoints  INTEGER NOT NULL,
    data       TEXT    NOT NULL   -- JSON array of SiteMapEntry
) STRICT;
