-- Schema v4: Organizer — requests the user keeps, with their own notes,
-- tags, status and severity. Messages are copied so items survive history
-- clean-ups.

CREATE TABLE organizer_items (
    id            INTEGER PRIMARY KEY,
    created_at    INTEGER NOT NULL,           -- unix epoch ms
    updated_at    INTEGER NOT NULL,
    exchange_id   INTEGER,                    -- origin, if it still exists
    name          TEXT    NOT NULL,
    notes         TEXT    NOT NULL DEFAULT '',
    tags          TEXT    NOT NULL DEFAULT '', -- comma-separated, lower case
    status        TEXT    NOT NULL,           -- todo | investigating | …
    severity      TEXT    NOT NULL,           -- none | info | low | …
    method        TEXT    NOT NULL,
    url           TEXT    NOT NULL,
    status_code   INTEGER,
    request_blob  INTEGER NOT NULL REFERENCES blobs(id),
    response_blob INTEGER REFERENCES blobs(id)
) STRICT;

CREATE INDEX idx_organizer_status ON organizer_items(status);
