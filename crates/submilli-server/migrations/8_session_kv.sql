-- Session key-value state. Keys and payloads are UTF-16 code units stored
-- big-endian, so comparing the bytes orders keys by code unit, as `list`
-- pages them. A payload is sealed when the server has a secret-store key and
-- `key_id` names the key that sealed it; without a key the payload is stored
-- as is and `key_id` is NULL. The byte counts are of the plaintext, as the
-- session limits count it. Closing a session deletes its rows in the same
-- transaction.
CREATE TABLE session_kv (
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    key BLOB NOT NULL CHECK(length(key) > 0 AND length(key) % 2 = 0),
    payload BLOB NOT NULL,
    key_id BLOB CHECK(key_id IS NULL OR length(key_id) = 8),
    value_bytes INTEGER NOT NULL CHECK(value_bytes >= 0),
    retained_bytes INTEGER NOT NULL CHECK(retained_bytes > value_bytes),
    PRIMARY KEY(session_id, key)
) WITHOUT ROWID;
