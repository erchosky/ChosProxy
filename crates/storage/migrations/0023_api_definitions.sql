-- Schema v23: imported API definitions (OpenAPI, Swagger, Postman, WSDL).
-- The endpoints are kept as JSON written by the engine.

CREATE TABLE api_definitions (
    id          INTEGER PRIMARY KEY,
    created_at  INTEGER NOT NULL,
    name        TEXT    NOT NULL,
    format      TEXT    NOT NULL,
    title       TEXT    NOT NULL DEFAULT '',
    version     TEXT    NOT NULL DEFAULT '',
    base_url    TEXT    NOT NULL DEFAULT '',
    endpoint_count INTEGER NOT NULL DEFAULT 0,
    warnings    TEXT    NOT NULL DEFAULT '[]',
    endpoints   TEXT    NOT NULL DEFAULT '[]'
);
