-- Keep lifecycle queries independent of encrypted programs and retained results.
-- Rebuild the child table too so foreign keys stay enabled throughout migration.
CREATE TABLE saved_outcomes AS SELECT * FROM durable_outcomes;
DROP TABLE durable_outcomes;
CREATE TABLE saved_programs AS SELECT run_id,execution_manifest AS content FROM runs WHERE execution_manifest IS NOT NULL;
CREATE TABLE saved_results AS SELECT run_id,response_status AS status,response_body AS content FROM runs WHERE response_body IS NOT NULL;
CREATE TABLE run_metadata (
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
    history_bytes INTEGER NOT NULL DEFAULT 0 CHECK(history_bytes >= 0),
    created_at_unix_ms INTEGER NOT NULL CHECK(created_at_unix_ms >= 0),
    state TEXT NOT NULL CHECK(state IN ('reserved','running','blocked','cancelled','aborted','completed','indeterminate','expired')),
    reason TEXT,
    completed_at_unix_ms INTEGER CHECK(completed_at_unix_ms >= 0),
    resources_retired INTEGER NOT NULL DEFAULT 0 CHECK(resources_retired IN (0,1)),
    UNIQUE(session_id, request_key),
    UNIQUE(session_id, admission_order),
    CHECK((state IN ('completed','expired') AND completed_at_unix_ms IS NOT NULL)
       OR (state NOT IN ('completed','expired') AND completed_at_unix_ms IS NULL)),
    CHECK(recovery_mode='unrecorded' OR state!='indeterminate'),
    CHECK(state != 'aborted' OR request_key IS NULL)
);
INSERT INTO run_metadata(run_id,reservation_id,execution_id,session_id,request_key,fingerprint,fingerprint_version,principal,entry_point,owner_generation,attempt,revision,admission_order,history_position,recovery_mode,history_bytes,created_at_unix_ms,state,reason,completed_at_unix_ms,resources_retired) SELECT run_id,reservation_id,execution_id,session_id,request_key,fingerprint,fingerprint_version,principal,entry_point,owner_generation,attempt,revision,admission_order,history_position,recovery_mode,history_bytes,created_at_unix_ms,CASE WHEN state='finished' THEN 'completed' ELSE state END,reason,completed_at_unix_ms,resources_retired FROM runs;
DROP TABLE runs;
ALTER TABLE run_metadata RENAME TO runs;
CREATE INDEX runs_by_completion ON runs(state,completed_at_unix_ms);
CREATE INDEX runs_by_startup ON runs(owner_generation,state);
CREATE INDEX runs_by_recovery ON runs(recovery_mode,resources_retired,run_id);
CREATE INDEX runs_by_session_state ON runs(session_id,state,admission_order);

CREATE TABLE run_programs (
    run_id TEXT PRIMARY KEY REFERENCES runs(run_id),
    content BLOB NOT NULL
);
INSERT INTO run_programs SELECT * FROM saved_programs;
DROP TABLE saved_programs;

CREATE TABLE run_results (
    run_id TEXT PRIMARY KEY REFERENCES runs(run_id),
    status INTEGER NOT NULL CHECK(status BETWEEN 100 AND 599),
    content BLOB NOT NULL,
    encrypted INTEGER NOT NULL CHECK(encrypted IN (0,1))
);
-- Old HTTP responses were plaintext. Preserve their exact bytes and encoding;
-- newly completed responses are sealed when the server has an encryption key.
INSERT INTO run_results SELECT run_id,status,content,0 FROM saved_results;
DROP TABLE saved_results;

CREATE TABLE durable_outcomes (
    run_id TEXT NOT NULL REFERENCES runs(run_id),
    sequence INTEGER NOT NULL CHECK(sequence >= 0),
    content BLOB NOT NULL,
    PRIMARY KEY(run_id,sequence)
);
INSERT INTO durable_outcomes SELECT * FROM saved_outcomes;
DROP TABLE saved_outcomes;
