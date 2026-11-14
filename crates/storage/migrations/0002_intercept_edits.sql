-- Schema v2: keep the original message when the user edited it during
-- interception (the main blobs hold what was actually sent/delivered).

ALTER TABLE exchanges ADD COLUMN original_request_blob  INTEGER REFERENCES blobs(id);
ALTER TABLE exchanges ADD COLUMN original_response_blob INTEGER REFERENCES blobs(id);
