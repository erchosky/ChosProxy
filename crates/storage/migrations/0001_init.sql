-- ChosProxy project schema v1.

CREATE TABLE project_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

-- Message bodies, stored apart from metadata so listings never load them.
CREATE TABLE blobs (
    id       INTEGER PRIMARY KEY,
    encoding INTEGER NOT NULL,          -- 0 = raw, 1 = zstd
    size     INTEGER NOT NULL,          -- uncompressed size in bytes
    data     BLOB    NOT NULL
) STRICT;

CREATE TABLE exchanges (
    id            INTEGER PRIMARY KEY,
    started_at    INTEGER NOT NULL,     -- unix epoch ms
    duration_ms   INTEGER,
    source        TEXT    NOT NULL,     -- proxy | repeater | ...
    listener      TEXT,
    client_addr   TEXT,
    scheme        TEXT    NOT NULL,
    method        TEXT    NOT NULL,
    host          TEXT    NOT NULL,
    port          INTEGER NOT NULL,
    path          TEXT    NOT NULL,     -- origin-form target incl. query
    extension     TEXT,
    url           TEXT    NOT NULL,
    http_version  TEXT    NOT NULL,
    status        INTEGER,
    reason        TEXT,
    mime          TEXT,
    request_len   INTEGER NOT NULL,     -- bytes on the wire
    response_len  INTEGER,
    server_ip     TEXT,
    tls           INTEGER NOT NULL DEFAULT 0,
    error         TEXT,
    notes         TEXT,
    flags         INTEGER NOT NULL DEFAULT 0,
    request_blob  INTEGER NOT NULL REFERENCES blobs(id),
    response_blob INTEGER REFERENCES blobs(id)
) STRICT;

CREATE INDEX idx_exchanges_host       ON exchanges(host);
CREATE INDEX idx_exchanges_status     ON exchanges(status);
CREATE INDEX idx_exchanges_method     ON exchanges(method);
CREATE INDEX idx_exchanges_mime       ON exchanges(mime);
CREATE INDEX idx_exchanges_extension  ON exchanges(extension);
CREATE INDEX idx_exchanges_source     ON exchanges(source);
CREATE INDEX idx_exchanges_started_at ON exchanges(started_at);
