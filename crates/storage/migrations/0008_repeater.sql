-- Schema v8: Repeater. A tab holds the request being edited and its target;
-- every send is stored in `exchanges` (source "repeater") and listed in the
-- tab's history through `repeater_sends`.

CREATE TABLE repeater_tabs (
    id                    INTEGER PRIMARY KEY,
    position              INTEGER NOT NULL,
    created_at            INTEGER NOT NULL,   -- unix epoch ms
    updated_at            INTEGER NOT NULL,
    name                  TEXT    NOT NULL,
    scheme                TEXT    NOT NULL,   -- http | https
    host                  TEXT    NOT NULL,
    port                  INTEGER NOT NULL,
    path                  TEXT    NOT NULL,   -- from the request line (for scope)
    request               TEXT    NOT NULL,   -- editor text, "\n" line endings
    head_crlf             INTEGER NOT NULL DEFAULT 1,
    body_crlf             INTEGER NOT NULL DEFAULT 1,
    update_content_length INTEGER NOT NULL DEFAULT 1
) STRICT;

CREATE TABLE repeater_sends (
    id          INTEGER PRIMARY KEY,
    tab_id      INTEGER NOT NULL REFERENCES repeater_tabs(id) ON DELETE CASCADE,
    exchange_id INTEGER NOT NULL,             -- the stored exchange
    sent_at     INTEGER NOT NULL,
    dns_ms      INTEGER NOT NULL DEFAULT 0,
    connect_ms  INTEGER NOT NULL DEFAULT 0,
    tls_ms      INTEGER NOT NULL DEFAULT 0,
    wait_ms     INTEGER NOT NULL DEFAULT 0,
    download_ms INTEGER NOT NULL DEFAULT 0,
    total_ms    INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX idx_repeater_sends_tab ON repeater_sends(tab_id, id);
