-- Keys are engine-owned tuples; package code supplies only the local UTF-16 key.
CREATE TABLE finalizer_artifacts (
    digest TEXT PRIMARY KEY NOT NULL,
    archive BLOB NOT NULL
);
CREATE TABLE session_finalizers (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(session_id),
    package TEXT NOT NULL,
    local_key BLOB NOT NULL CHECK(length(local_key)>0 AND length(local_key)%2=0),
    artifact_digest TEXT REFERENCES finalizer_artifacts(digest),
    payload BLOB,
    sealed INTEGER NOT NULL CHECK(sealed IN (0,1)),
    retained_bytes INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('registered','pending','running','succeeded','failed')),
    failure TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    started_at TEXT,
    attempt_owner TEXT,
    completed_at TEXT
);
CREATE UNIQUE INDEX live_finalizer_keys ON session_finalizers(session_id,package,local_key)
    WHERE status IN ('registered','pending','running');
CREATE INDEX finalizers_by_artifact ON session_finalizers(artifact_digest);
CREATE INDEX finalizers_for_session ON session_finalizers(session_id,status);
-- A terminating session's credentials are needed until cleanup completes.
CREATE TABLE finalizer_context (
    session_id TEXT PRIMARY KEY NOT NULL REFERENCES sessions(session_id),
    encrypted_harness_bindings BLOB
);
