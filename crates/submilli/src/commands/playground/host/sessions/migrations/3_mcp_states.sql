-- The MCP `initialize` parameters of each open agent session, as JSON, so the
-- session resumes under the same id after a restart. Never holds a secret: the
-- host strips `_meta.secrets` before storing it.
CREATE TABLE mcp_states (
    session_id TEXT PRIMARY KEY NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    initialize_params TEXT NOT NULL
) WITHOUT ROWID;
