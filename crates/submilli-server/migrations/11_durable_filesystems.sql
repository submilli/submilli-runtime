-- Idempotent filesystem operations. The caller supplies a stable scope and
-- operation identity, configured mounts, and input bytes on retry. No run,
-- workspace lifecycle, execution history, or multi-step plan is stored here.
CREATE TABLE durable_filesystem_operations (
    scope_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    version INTEGER NOT NULL CHECK(version > 0),
    request BLOB NOT NULL,
    state BLOB,
    -- Derived from the encoded state, for pending admission without decrypting rows.
    status TEXT NOT NULL CHECK(status IN ('preparing', 'pending', 'done', 'rejected')),
    PRIMARY KEY(scope_id, operation_id),
    CHECK((status = 'preparing' AND state IS NULL)
       OR (status <> 'preparing' AND state IS NOT NULL))
);
-- Serialize unfinished filesystem mutations within the caller's scope.
CREATE UNIQUE INDEX durable_filesystem_operation_pending ON durable_filesystem_operations(scope_id)
    WHERE status IN ('preparing', 'pending');

CREATE TABLE durable_filesystem_readers (
    scope_id TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    request BLOB NOT NULL,
    position BLOB,
    error BLOB,
    progress BLOB,
    PRIMARY KEY(scope_id, resource_id)
);

CREATE TABLE durable_filesystem_listings (
    scope_id TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    request BLOB NOT NULL,
    content BLOB,
    error BLOB,
    progress BLOB,
    PRIMARY KEY(scope_id, resource_id)
);

CREATE TABLE durable_filesystem_writers (
    scope_id TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    request BLOB NOT NULL,
    close_effect TEXT,
    state TEXT NOT NULL CHECK(state IN ('open','closing','closed')),
    PRIMARY KEY(scope_id, resource_id)
);
CREATE TABLE durable_filesystem_chunks (
    scope_id TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    chunk_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    content BLOB NOT NULL,
    PRIMARY KEY(scope_id, resource_id, chunk_id),
    UNIQUE(scope_id, resource_id, sequence),
    FOREIGN KEY(scope_id, resource_id) REFERENCES durable_filesystem_writers(scope_id, resource_id)
);
