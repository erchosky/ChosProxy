-- Schema v5: WebSocket connections and messages.

-- The handshake exchange of a WebSocket connection carries its key.
ALTER TABLE exchanges ADD COLUMN ws_key INTEGER;

CREATE TABLE ws_connections (
    id           INTEGER PRIMARY KEY,
    conn_key     INTEGER NOT NULL UNIQUE,
    started_at   INTEGER NOT NULL,          -- unix epoch ms
    ended_at     INTEGER,
    source       TEXT    NOT NULL,
    scheme       TEXT    NOT NULL,
    host         TEXT    NOT NULL,
    port         INTEGER NOT NULL,
    path         TEXT    NOT NULL,          -- path + query of the handshake
    url          TEXT    NOT NULL,
    close_code   INTEGER,
    close_reason TEXT,
    messages     INTEGER NOT NULL DEFAULT 0,
    bytes        INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE ws_messages (
    id            INTEGER PRIMARY KEY,
    connection_id INTEGER NOT NULL REFERENCES ws_connections(id),
    at            INTEGER NOT NULL,         -- unix epoch ms
    direction     INTEGER NOT NULL,         -- 0 client→server, 1 server→client
    opcode        INTEGER NOT NULL,
    length        INTEGER NOT NULL,         -- payload length as delivered
    payload_blob  INTEGER NOT NULL REFERENCES blobs(id),
    original_blob INTEGER REFERENCES blobs(id),
    flags         INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX idx_ws_messages_connection ON ws_messages(connection_id, id);
CREATE INDEX idx_exchanges_ws_key ON exchanges(ws_key) WHERE ws_key IS NOT NULL;
