-- Schema v14: ChosOAST (ADR-0026). Payloads created in this project, the
-- interactions received for them, and which stored requests carried them.

CREATE TABLE oast_payloads (
    id             INTEGER PRIMARY KEY,
    correlation_id TEXT    NOT NULL UNIQUE,
    created_at     INTEGER NOT NULL,
    note           TEXT    NOT NULL DEFAULT '',
    dns_name       TEXT,
    url            TEXT    NOT NULL
) STRICT;

CREATE TABLE oast_interactions (
    id             INTEGER PRIMARY KEY,
    correlation_id TEXT    NOT NULL,
    received_at    INTEGER NOT NULL,
    protocol       TEXT    NOT NULL,
    remote         TEXT    NOT NULL,
    summary        TEXT    NOT NULL,
    raw_blob       INTEGER NOT NULL REFERENCES blobs(id),
    -- Identifies the interaction on its server, so a poll repeated after a
    -- failure does not store it twice.
    server_key     TEXT    NOT NULL
) STRICT;

CREATE UNIQUE INDEX idx_oast_interactions_key ON oast_interactions(server_key, correlation_id);
CREATE INDEX idx_oast_interactions_id ON oast_interactions(correlation_id, id);

CREATE TABLE oast_links (
    correlation_id TEXT    NOT NULL,
    exchange_id    INTEGER NOT NULL,
    PRIMARY KEY (correlation_id, exchange_id)
) STRICT;

CREATE TABLE oast_state (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    link_cursor   INTEGER NOT NULL DEFAULT 0,
    remote_server TEXT    NOT NULL DEFAULT '',
    remote_seq    INTEGER NOT NULL DEFAULT 0
) STRICT;

INSERT INTO oast_state (id) VALUES (1);
