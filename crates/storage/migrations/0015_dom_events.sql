-- Schema v15: DOM Inspector observations (ADR-0027), merged by dedup key.

CREATE TABLE dom_events (
    id             INTEGER PRIMARY KEY,
    first_at       INTEGER NOT NULL,
    last_at        INTEGER NOT NULL,
    hits           INTEGER NOT NULL DEFAULT 1,
    kind           TEXT    NOT NULL,
    sink           TEXT    NOT NULL,
    source         TEXT,
    source_value   TEXT,
    matched        TEXT,
    transformation TEXT,
    value          TEXT    NOT NULL,
    canary         INTEGER NOT NULL DEFAULT 0,
    checks_origin  INTEGER,
    frame_url      TEXT    NOT NULL,
    top_frame      INTEGER NOT NULL DEFAULT 1,
    location       TEXT,
    stack          TEXT    NOT NULL DEFAULT '',
    exchange_id    INTEGER,
    dedup_key      TEXT    NOT NULL UNIQUE
) STRICT;

CREATE INDEX idx_dom_events_last ON dom_events(last_at);
