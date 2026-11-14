-- Schema v24: a GraphQL definition keeps the schema read by introspection
-- (JSON written by the engine; empty for other formats).

ALTER TABLE api_definitions ADD COLUMN schema TEXT NOT NULL DEFAULT '';
