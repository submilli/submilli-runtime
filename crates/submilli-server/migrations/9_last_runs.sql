-- The last run of each active session, as the `{result, console, error}` JSON
-- that `GET /v1/sessions/{id}/last-run` answers with. A run writes it in the
-- transaction that records the run's end, so the latest is the last run to
-- end. The JSON is sealed when the server has a secret-store key and `key_id`
-- names the key that sealed it; without a key it is stored as is and `key_id`
-- is NULL. Over the inline limit, the sealed JSON is the file `spill_file` in
-- the session's directory beside the database; over the retained limit only
-- its size is kept, with the limit it was over. Closing a session deletes its
-- row in the same transaction. Last runs before this table were held in
-- memory and are gone.
CREATE TABLE last_runs (
    session_id TEXT PRIMARY KEY NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    completed_at_unix_ms INTEGER NOT NULL CHECK(completed_at_unix_ms >= 0),
    storage TEXT NOT NULL CHECK(storage IN ('inline', 'file', 'too_large')),
    -- The length of the plaintext JSON, as the limits count it.
    encoded_bytes INTEGER NOT NULL CHECK(encoded_bytes >= 0),
    payload BLOB,
    spill_file TEXT CHECK(spill_file IS NULL OR length(spill_file) = 36),
    key_id BLOB CHECK(key_id IS NULL OR length(key_id) = 8),
    limit_bytes INTEGER CHECK(limit_bytes >= 0),
    CHECK((storage = 'inline' AND payload IS NOT NULL AND spill_file IS NULL AND limit_bytes IS NULL)
       OR (storage = 'file' AND payload IS NULL AND spill_file IS NOT NULL AND limit_bytes IS NULL)
       OR (storage = 'too_large' AND payload IS NULL AND spill_file IS NULL AND key_id IS NULL
           AND limit_bytes IS NOT NULL))
) WITHOUT ROWID;
