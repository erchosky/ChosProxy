-- Schema v3 (Target): row highlights and site map marks.

ALTER TABLE exchanges ADD COLUMN highlight TEXT;

-- A mark on a site map node: an origin plus a path that is either an
-- endpoint (`/a/b`) or a folder (`/a/`).
CREATE TABLE site_marks (
    scheme TEXT    NOT NULL,
    host   TEXT    NOT NULL,
    port   INTEGER NOT NULL,
    path   TEXT    NOT NULL,
    mark   TEXT    NOT NULL,          -- interesting | reviewed | ignore | favorite
    PRIMARY KEY (scheme, host, port, path)
) STRICT;

CREATE INDEX idx_exchanges_highlight ON exchanges(highlight) WHERE highlight IS NOT NULL;
