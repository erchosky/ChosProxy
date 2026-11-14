-- Schema v9: Repeater tab groups, send options and the project cookie jar.

CREATE TABLE repeater_groups (
    id          INTEGER PRIMARY KEY,
    position    INTEGER NOT NULL,
    created_at  INTEGER NOT NULL,   -- unix epoch ms
    name        TEXT    NOT NULL,
    color       TEXT    NOT NULL,   -- one of the highlight colour names
    send_mode   TEXT    NOT NULL DEFAULT 'sequence'  -- sequence | single | parallel
) STRICT;

ALTER TABLE repeater_tabs ADD COLUMN group_id INTEGER REFERENCES repeater_groups(id) ON DELETE SET NULL;
ALTER TABLE repeater_tabs ADD COLUMN reuse_connection INTEGER NOT NULL DEFAULT 0;
ALTER TABLE repeater_tabs ADD COLUMN use_cookie_jar INTEGER NOT NULL DEFAULT 0;

ALTER TABLE repeater_sends ADD COLUMN reused INTEGER NOT NULL DEFAULT 0;
ALTER TABLE repeater_sends ADD COLUMN jar_cookies TEXT NOT NULL DEFAULT '';  -- names, space separated

-- Cookies set by responses (proxy and Repeater traffic), RFC 6265 storage
-- model. A cookie is identified by name, domain and path.
CREATE TABLE cookie_jar (
    id          INTEGER PRIMARY KEY,
    domain      TEXT    NOT NULL,
    host_only   INTEGER NOT NULL,
    path        TEXT    NOT NULL,
    name        TEXT    NOT NULL,
    value       TEXT    NOT NULL,
    expires_at  INTEGER,            -- unix epoch ms; NULL = session cookie
    secure      INTEGER NOT NULL DEFAULT 0,
    http_only   INTEGER NOT NULL DEFAULT 0,
    same_site   TEXT,
    source      TEXT    NOT NULL,   -- proxy | repeater | user
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    UNIQUE (name, domain, path)
) STRICT;

CREATE INDEX idx_cookie_jar_domain ON cookie_jar(domain);
