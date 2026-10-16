-- Shared effect records and filesystem workspaces/resources. The execution
-- history integration will use the same run-scoped effect identities.
CREATE TABLE durable_workspaces (
    workspace_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL UNIQUE REFERENCES runs(run_id),
    session_id TEXT NOT NULL,
    owner TEXT NOT NULL,
    identity BLOB,
    layout BLOB,
    reservation BLOB,
    state TEXT NOT NULL CHECK(state IN ('preparing', 'active', 'retiring', 'gone')),
    CHECK(state != 'active' OR (identity IS NOT NULL AND layout IS NOT NULL))
);
CREATE INDEX durable_workspaces_by_session ON durable_workspaces(session_id, state);

-- Shared effect lifecycle. Requests and outcomes are immutable once recorded;
-- recovery_payload belongs to the operation adapter. Payload bytes are sealed
-- before SQL; large file contents are references into the object store.
-- Execution history sequencing, attempt fencing and accounting are separate
-- integration work. These rows are not an append-only execution event stream.
CREATE TABLE durable_effects (
    run_id TEXT NOT NULL REFERENCES runs(run_id),
    effect_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    version INTEGER NOT NULL CHECK(version > 0),
    request BLOB NOT NULL,
    recovery_payload BLOB,
    outcome BLOB,
    state TEXT NOT NULL CHECK(state IN ('preparing', 'pending', 'done', 'rejected')),
    PRIMARY KEY(run_id, effect_id),
    CHECK((state = 'preparing' AND recovery_payload IS NULL AND outcome IS NULL)
       OR (state = 'pending' AND recovery_payload IS NOT NULL AND outcome IS NULL)
       OR (state = 'done' AND recovery_payload IS NOT NULL AND outcome IS NOT NULL)
       OR (state = 'rejected' AND recovery_payload IS NULL AND outcome IS NOT NULL))
);
-- One unfinished effect per run, regardless of its operation adapter.
CREATE UNIQUE INDEX durable_effect_pending ON durable_effects(run_id)
    WHERE state IN ('preparing', 'pending');

CREATE TABLE durable_filesystem_plans (
    workspace_id TEXT NOT NULL REFERENCES durable_workspaces(workspace_id),
    plan_id TEXT NOT NULL,
    request BLOB NOT NULL,
    outcome BLOB,
    PRIMARY KEY(workspace_id, plan_id)
);
CREATE UNIQUE INDEX durable_filesystem_pending_plan ON durable_filesystem_plans(workspace_id) WHERE outcome IS NULL;

CREATE TABLE durable_filesystem_readers (
    workspace_id TEXT NOT NULL REFERENCES durable_workspaces(workspace_id),
    resource_id TEXT NOT NULL,
    request BLOB NOT NULL,
    position BLOB,
    error BLOB,
    PRIMARY KEY(workspace_id, resource_id)
);

CREATE TABLE durable_filesystem_objects (
    workspace_id TEXT NOT NULL REFERENCES durable_workspaces(workspace_id),
    object_id TEXT NOT NULL,
    request BLOB NOT NULL,
    PRIMARY KEY(workspace_id, object_id)
);

CREATE TABLE durable_filesystem_listings (
    workspace_id TEXT NOT NULL REFERENCES durable_workspaces(workspace_id),
    resource_id TEXT NOT NULL,
    request BLOB NOT NULL,
    content BLOB,
    error BLOB,
    PRIMARY KEY(workspace_id, resource_id)
);

CREATE TABLE durable_filesystem_writers (
    workspace_id TEXT NOT NULL REFERENCES durable_workspaces(workspace_id),
    resource_id TEXT NOT NULL,
    request BLOB NOT NULL,
    close_effect TEXT,
    state TEXT NOT NULL CHECK(state IN ('open','closing','closed')),
    PRIMARY KEY(workspace_id, resource_id)
);
CREATE TABLE durable_filesystem_chunks (
    workspace_id TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    chunk_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    content BLOB NOT NULL,
    PRIMARY KEY(workspace_id, resource_id, chunk_id),
    UNIQUE(workspace_id, resource_id, sequence),
    FOREIGN KEY(workspace_id, resource_id) REFERENCES durable_filesystem_writers(workspace_id, resource_id)
);
