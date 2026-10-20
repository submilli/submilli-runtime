-- A closing session stays here, no longer open, until its finalizers have run
-- and its files are gone.
ALTER TABLE sessions ADD COLUMN closing INTEGER NOT NULL DEFAULT 0 CHECK(closing IN (0,1));
-- Package finalizers a session's runs registered. Keys are the package's own
-- UTF-16 key, stored big-endian; the engine supplies the package. Arguments are
-- sealed with the playground's session key and dropped once the attempt ends.
-- A finished row outlives its session, so its outcome can still be read.
CREATE TABLE session_finalizers (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    package TEXT NOT NULL,
    local_key BLOB NOT NULL CHECK(length(local_key)>0 AND length(local_key)%2=0),
    package_versions TEXT,
    payload BLOB,
    retained_bytes INTEGER NOT NULL CHECK(retained_bytes >= 0),
    status TEXT NOT NULL CHECK(status IN ('registered','pending','running','succeeded','failed')),
    failure TEXT
);
CREATE UNIQUE INDEX live_finalizer_keys ON session_finalizers(session_id,package,local_key)
    WHERE status IN ('registered','pending','running');
CREATE INDEX finalizers_for_session ON session_finalizers(session_id,status);
-- A closing session's secret bindings, sealed, kept only while one of its
-- finalizers has yet to run, so a finalizer run after a restart can still use
-- them.
CREATE TABLE finalizer_context (
    session_id TEXT PRIMARY KEY NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    sealed_secrets BLOB NOT NULL
);
