-- Schema v7: findings. A finding describes one problem; it holds one or more
-- pieces of evidence (copies of the requests/responses that show it).

CREATE TABLE findings (
    id          INTEGER PRIMARY KEY,
    created_at  INTEGER NOT NULL,         -- unix epoch ms
    updated_at  INTEGER NOT NULL,
    source      TEXT    NOT NULL,         -- manual | (later: passive, scanner, plugin)
    dedup_key   TEXT    UNIQUE,           -- equal keys are merged (automatic sources)
    kind        TEXT    NOT NULL,         -- type, e.g. "Missing security header"
    title       TEXT    NOT NULL,
    severity    TEXT    NOT NULL,         -- info | low | medium | high | critical
    confidence  TEXT    NOT NULL,         -- tentative | firm | certain
    status      TEXT    NOT NULL,         -- open | confirmed | falsePositive | acceptedRisk | fixed
    scheme      TEXT    NOT NULL,
    host        TEXT    NOT NULL,
    port        INTEGER NOT NULL,
    path        TEXT    NOT NULL,
    url         TEXT    NOT NULL,
    description TEXT    NOT NULL DEFAULT '',
    remediation TEXT    NOT NULL DEFAULT '',
    notes       TEXT    NOT NULL DEFAULT ''
) STRICT;

CREATE TABLE finding_evidence (
    id            INTEGER PRIMARY KEY,
    finding_id    INTEGER NOT NULL REFERENCES findings(id),
    created_at    INTEGER NOT NULL,
    exchange_id   INTEGER,                -- origin in the history, if any
    note          TEXT    NOT NULL DEFAULT '',
    method        TEXT    NOT NULL,
    url           TEXT    NOT NULL,
    status_code   INTEGER,
    request_blob  INTEGER NOT NULL REFERENCES blobs(id),
    response_blob INTEGER REFERENCES blobs(id)
) STRICT;

CREATE INDEX idx_finding_evidence ON finding_evidence(finding_id);
CREATE UNIQUE INDEX idx_finding_evidence_exchange
    ON finding_evidence(finding_id, exchange_id) WHERE exchange_id IS NOT NULL;
