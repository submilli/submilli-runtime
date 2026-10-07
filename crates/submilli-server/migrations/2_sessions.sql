CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY NOT NULL,
    -- Version of this session row, used to reject stale writes.
    record_version INTEGER NOT NULL CHECK (record_version > 0),
    status TEXT NOT NULL CHECK (status IN ('active', 'closed')),
    closed_reason TEXT CHECK (closed_reason IN ('deleted', 'expired', 'blueprint_removed')),
    blueprint_name TEXT NOT NULL,
    idle_timeout_ms INTEGER NOT NULL CHECK (idle_timeout_ms >= 0),
    last_activity_unix_ms INTEGER NOT NULL CHECK (last_activity_unix_ms >= 0),
    -- NULL preserves an unknown legacy mode until first use resolves it.
    root_vfs_type TEXT CHECK (root_vfs_type IN ('none', 'ephemeral', 'per_session', 'named')),
    root_vfs_path TEXT,
    encrypted_harness_bindings BLOB,
    CHECK (status = 'closed' OR closed_reason IS NULL),
    CHECK (root_vfs_type != 'named' OR (root_vfs_path IS NOT NULL AND length(root_vfs_path) > 0))
);
-- No foreign key: recovery can schedule an orphan folder without inventing a session.
-- Until SUB-1333, tasks also retry the external idempotency ledger purge.
CREATE TABLE session_cleanup (
    session_id TEXT PRIMARY KEY NOT NULL,
    folder_path TEXT
);
CREATE INDEX sessions_by_blueprint ON sessions(blueprint_name, status);
CREATE TABLE session_variables (
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (session_id, name)
);
CREATE TABLE session_mcp (
    session_id TEXT PRIMARY KEY NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    protocol_version TEXT NOT NULL,
    client_name TEXT NOT NULL,
    client_version TEXT NOT NULL,
    client_title TEXT,
    client_description TEXT,
    client_website_url TEXT,
    client_icons_present INTEGER NOT NULL CHECK (client_icons_present IN (0, 1)),
    -- These are extensible MCP wire objects, not session lifecycle records.
    capabilities_json TEXT NOT NULL CHECK (json_valid(capabilities_json)),
    metadata_json TEXT CHECK (metadata_json IS NULL OR json_valid(metadata_json))
);
CREATE TABLE session_mcp_icons (
    session_id TEXT NOT NULL REFERENCES session_mcp(session_id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    src TEXT NOT NULL,
    mime_type TEXT,
    theme TEXT CHECK (theme IS NULL OR theme IN ('light', 'dark')),
    sizes_present INTEGER NOT NULL CHECK (sizes_present IN (0, 1)),
    PRIMARY KEY (session_id, position)
);
CREATE TABLE session_mcp_icon_sizes (
    session_id TEXT NOT NULL,
    icon_position INTEGER NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    size TEXT NOT NULL,
    PRIMARY KEY (session_id, icon_position, position),
    FOREIGN KEY (session_id, icon_position) REFERENCES session_mcp_icons(session_id, position) ON DELETE CASCADE
);
