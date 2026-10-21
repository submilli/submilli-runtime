-- The project's open sessions. A closed session's row is deleted, and its
-- variables, values and last run go with it.
CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY NOT NULL,
    blueprint_name TEXT NOT NULL,
    idle_timeout_ms INTEGER NOT NULL CHECK (idle_timeout_ms >= 0),
    last_activity_unix_ms INTEGER NOT NULL CHECK (last_activity_unix_ms >= 0)
);
-- The variables bound when the session opened, defaults filled in. Fixed for
-- the session's life.
CREATE TABLE session_variables (
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (session_id, name)
);
-- Session key-value state. Keys and payloads are UTF-16 code units stored
-- big-endian, so comparing the bytes orders keys by code unit, as `list`
-- pages them. Every payload is sealed with the playground's session key. The
-- byte counts are of the plaintext, as the session limits count it.
CREATE TABLE session_kv (
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    key BLOB NOT NULL CHECK(length(key) > 0 AND length(key) % 2 = 0),
    payload BLOB NOT NULL,
    value_bytes INTEGER NOT NULL CHECK(value_bytes >= 0),
    retained_bytes INTEGER NOT NULL CHECK(retained_bytes > value_bytes),
    PRIMARY KEY(session_id, key)
) WITHOUT ROWID;
-- The last run of each open session, as sealed `{result, console, error}`
-- JSON. Over the retained limit only its size is kept, with the limit it was
-- over.
CREATE TABLE last_runs (
    session_id TEXT PRIMARY KEY NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    completed_at_unix_ms INTEGER NOT NULL CHECK(completed_at_unix_ms >= 0),
    encoded_bytes INTEGER NOT NULL CHECK(encoded_bytes >= 0),
    payload BLOB,
    limit_bytes INTEGER CHECK(limit_bytes >= 0),
    CHECK((payload IS NULL) != (limit_bytes IS NULL))
) WITHOUT ROWID;
