-- The logical node belongs to the store, not to a process or hostname.
CREATE TABLE execution_node (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    node_id TEXT NOT NULL UNIQUE
);
INSERT INTO execution_node VALUES (1, lower(hex(randomblob(16))));

CREATE TABLE session_run_sequences (
    session_id TEXT PRIMARY KEY,
    last_order INTEGER NOT NULL CHECK(last_order > 0)
);

-- No cascading session deletion: closing a session cannot erase evidence of
-- work that may have executed. Payload/history retention is managed separately.
CREATE TABLE runs (
    run_id TEXT PRIMARY KEY,
    reservation_id TEXT NOT NULL UNIQUE,
    execution_id TEXT,
    session_id TEXT NOT NULL,
    request_key TEXT CHECK(request_key IS NULL OR length(CAST(request_key AS BLOB)) BETWEEN 1 AND 120),
    fingerprint TEXT NOT NULL,
    fingerprint_version INTEGER NOT NULL DEFAULT 1 CHECK(fingerprint_version = 1),
    principal TEXT,
    entry_point TEXT NOT NULL DEFAULT 'legacy',
    origin_node TEXT NOT NULL,
    owner_generation TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt >= 0),
    revision INTEGER NOT NULL DEFAULT 0 CHECK(revision >= 0),
    admission_order INTEGER NOT NULL CHECK(admission_order > 0),
    history_position INTEGER NOT NULL DEFAULT 0 CHECK(history_position = 0),
    recovery_mode TEXT NOT NULL DEFAULT 'unrecorded' CHECK(recovery_mode = 'unrecorded'),
    created_at_unix_ms INTEGER NOT NULL CHECK(created_at_unix_ms >= 0),
    state TEXT NOT NULL CHECK(state IN ('reserved','running','blocked','cancelled','aborted','completed','finished','indeterminate','expired')),
    reason TEXT,
    response_status INTEGER,
    response_body BLOB,
    completed_at_unix_ms INTEGER CHECK(completed_at_unix_ms >= 0),
    UNIQUE(session_id, request_key),
    UNIQUE(session_id, admission_order),
    CHECK((state = 'completed' AND response_status BETWEEN 100 AND 599 AND response_status IS NOT NULL AND response_body IS NOT NULL AND completed_at_unix_ms IS NOT NULL)
       OR (state IN ('expired','finished') AND response_status IS NULL AND response_body IS NULL AND completed_at_unix_ms IS NOT NULL)
       OR (state NOT IN ('completed','expired','finished') AND response_status IS NULL AND response_body IS NULL AND completed_at_unix_ms IS NULL)),
    CHECK(state != 'finished' OR request_key IS NULL),
    CHECK(state != 'aborted' OR request_key IS NULL)
);
INSERT INTO runs(run_id,reservation_id,execution_id,session_id,request_key,fingerprint,origin_node,owner_generation,admission_order,created_at_unix_ms,state,reason,response_status,response_body,completed_at_unix_ms)
SELECT reservation_id,reservation_id,
       CASE WHEN json_valid(CAST(response_body AS TEXT)) THEN
           CASE WHEN json_type(CAST(response_body AS TEXT),'$.execution_id')='text'
                THEN json_extract(CAST(response_body AS TEXT),'$.execution_id') END END,
       session_id,request_key,fingerprint,(SELECT node_id FROM execution_node),owner_generation,
       row_number() OVER (PARTITION BY session_id ORDER BY created_at_unix_ms,reservation_id),
       created_at_unix_ms,CASE WHEN state='reserved' THEN 'indeterminate' ELSE state END,
       CASE WHEN state IN ('reserved','indeterminate') THEN 'legacy_without_history' END,
       response_status,response_body,completed_at_unix_ms
FROM idempotent_requests;
INSERT INTO session_run_sequences SELECT session_id,MAX(admission_order) FROM runs GROUP BY session_id;
DROP TABLE idempotent_requests;
CREATE INDEX runs_by_completion ON runs(state,completed_at_unix_ms);
CREATE INDEX runs_by_startup ON runs(owner_generation,state);
