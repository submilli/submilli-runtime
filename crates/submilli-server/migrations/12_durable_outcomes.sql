-- Preserve admission identities and tombstones while enabling ordered history.
CREATE TABLE recorded_runs (
    run_id TEXT PRIMARY KEY,
    reservation_id TEXT NOT NULL UNIQUE,
    execution_id TEXT,
    session_id TEXT NOT NULL,
    request_key TEXT CHECK(request_key IS NULL OR length(CAST(request_key AS BLOB)) BETWEEN 1 AND 120),
    fingerprint TEXT NOT NULL,
    fingerprint_version INTEGER NOT NULL DEFAULT 1 CHECK(fingerprint_version = 1),
    principal TEXT,
    entry_point TEXT NOT NULL DEFAULT 'legacy',
    owner_generation TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt >= 0),
    revision INTEGER NOT NULL DEFAULT 0 CHECK(revision >= 0),
    admission_order INTEGER NOT NULL CHECK(admission_order > 0),
    history_position INTEGER NOT NULL DEFAULT 0 CHECK(history_position >= 0),
    recovery_mode TEXT NOT NULL DEFAULT 'unrecorded' CHECK(recovery_mode IN ('unrecorded','recorded')),
    execution_manifest BLOB,
    history_bytes INTEGER NOT NULL DEFAULT 0 CHECK(history_bytes >= 0),
    history_host_fuel INTEGER NOT NULL DEFAULT 0 CHECK(history_host_fuel >= 0),
    created_at_unix_ms INTEGER NOT NULL CHECK(created_at_unix_ms >= 0),
    state TEXT NOT NULL CHECK(state IN ('reserved','running','blocked','cancelled','aborted','completed','finished','indeterminate','expired')),
    reason TEXT,
    response_status INTEGER,
    response_body BLOB,
    completed_at_unix_ms INTEGER CHECK(completed_at_unix_ms >= 0),
    CHECK(recovery_mode='unrecorded' OR execution_manifest IS NOT NULL),
    UNIQUE(session_id, request_key),
    UNIQUE(session_id, admission_order),
    CHECK((state = 'completed' AND response_status BETWEEN 100 AND 599 AND response_status IS NOT NULL AND response_body IS NOT NULL AND completed_at_unix_ms IS NOT NULL)
       OR (state IN ('expired','finished') AND response_status IS NULL AND response_body IS NULL AND completed_at_unix_ms IS NOT NULL)
       OR (state NOT IN ('completed','expired','finished') AND response_status IS NULL AND response_body IS NULL AND completed_at_unix_ms IS NULL)),
    CHECK(state != 'finished' OR request_key IS NULL),
    CHECK(state != 'aborted' OR request_key IS NULL)
);
INSERT INTO recorded_runs(run_id,reservation_id,execution_id,session_id,request_key,fingerprint,fingerprint_version,principal,entry_point,owner_generation,attempt,revision,admission_order,history_position,recovery_mode,created_at_unix_ms,state,reason,response_status,response_body,completed_at_unix_ms) SELECT run_id,reservation_id,execution_id,session_id,request_key,fingerprint,fingerprint_version,principal,entry_point,owner_generation,attempt,revision,admission_order,history_position,recovery_mode,created_at_unix_ms,state,reason,response_status,response_body,completed_at_unix_ms FROM runs;
DROP TABLE runs;
ALTER TABLE recorded_runs RENAME TO runs;
CREATE INDEX runs_by_completion ON runs(state,completed_at_unix_ms);
CREATE INDEX runs_by_startup ON runs(owner_generation,state);

-- Immutable completed outcomes only. Filesystem retry journals remain separate.
CREATE TABLE durable_outcomes (
    run_id TEXT NOT NULL REFERENCES runs(run_id),
    sequence INTEGER NOT NULL CHECK(sequence >= 0),
    content BLOB NOT NULL,
    PRIMARY KEY(run_id, sequence)
);
