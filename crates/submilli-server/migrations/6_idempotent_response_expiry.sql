-- A completed response is kept for a while, then expires: its body is dropped
-- and the row stays as a tombstone, so the request is still never run twice.
-- SQLite cannot change a CHECK in place, so the table is rebuilt.
CREATE TABLE idempotent_requests_new (
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    request_key TEXT NOT NULL CHECK(length(CAST(request_key AS BLOB)) BETWEEN 1 AND 120),
    fingerprint TEXT NOT NULL,
    reservation_id TEXT NOT NULL UNIQUE,
    owner_generation TEXT NOT NULL,
    created_at_unix_ms INTEGER NOT NULL CHECK(created_at_unix_ms >= 0),
    state TEXT NOT NULL CHECK(state IN ('reserved', 'completed', 'indeterminate', 'expired')),
    response_status INTEGER,
    response_body BLOB,
    completed_at_unix_ms INTEGER CHECK(completed_at_unix_ms >= 0),
    PRIMARY KEY(session_id, request_key),
    CHECK((state = 'completed' AND response_status IS NOT NULL AND response_status BETWEEN 100 AND 599 AND response_body IS NOT NULL AND completed_at_unix_ms IS NOT NULL)
       OR (state = 'expired' AND response_status IS NOT NULL AND response_status BETWEEN 100 AND 599 AND response_body IS NULL AND completed_at_unix_ms IS NOT NULL)
       OR (state IN ('reserved', 'indeterminate') AND response_status IS NULL AND response_body IS NULL AND completed_at_unix_ms IS NULL))
);

INSERT INTO idempotent_requests_new(session_id, request_key, fingerprint, reservation_id, owner_generation, created_at_unix_ms, state, response_status, response_body, completed_at_unix_ms)
SELECT session_id, request_key, fingerprint, reservation_id, owner_generation, created_at_unix_ms, state, response_status, response_body,
       CASE WHEN state = 'completed' THEN created_at_unix_ms END
FROM idempotent_requests;

DROP TABLE idempotent_requests;
ALTER TABLE idempotent_requests_new RENAME TO idempotent_requests;

CREATE INDEX idempotent_requests_by_completion ON idempotent_requests(state, completed_at_unix_ms);
