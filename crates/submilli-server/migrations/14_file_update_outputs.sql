-- The native result is captured in the same transaction as prepared publication.
-- Retiring the operation also retires its encrypted result.
CREATE TABLE durable_file_update_outputs (
    scope_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    bytes INTEGER NOT NULL CHECK(bytes >= 0 AND bytes <= 67108864),
    output BLOB NOT NULL,
    PRIMARY KEY(scope_id, operation_id),
    FOREIGN KEY(scope_id, operation_id)
        REFERENCES durable_filesystem_operations(scope_id, operation_id) ON DELETE CASCADE
);
