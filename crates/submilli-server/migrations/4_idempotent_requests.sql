CREATE TABLE idempotent_requests (
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    request_key TEXT NOT NULL CHECK(length(CAST(request_key AS BLOB)) BETWEEN 1 AND 120),
    fingerprint TEXT NOT NULL,
    reservation_id TEXT NOT NULL UNIQUE,
    owner_generation TEXT NOT NULL,
    created_at_unix_ms INTEGER NOT NULL CHECK(created_at_unix_ms >= 0),
    state TEXT NOT NULL CHECK(state IN ('reserved', 'completed', 'indeterminate')),
    response_status INTEGER,
    response_body BLOB,
    PRIMARY KEY(session_id, request_key),
    CHECK((state = 'completed' AND response_status IS NOT NULL AND response_status BETWEEN 100 AND 599 AND response_body IS NOT NULL)
       OR (state != 'completed' AND response_status IS NULL AND response_body IS NULL))
);
