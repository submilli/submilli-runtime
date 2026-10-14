-- Operator secrets and MCP OAuth grants. Every value is sealed before it reaches
-- SQL: a format byte, the id of the key that sealed it, then the ciphertext,
-- bound to its row's name.
CREATE TABLE secrets (
    name TEXT PRIMARY KEY NOT NULL CHECK(length(CAST(name AS BLOB)) BETWEEN 1 AND 512 AND substr(name, 1, 10) != 'mcp_oauth/'),
    sealed_value BLOB NOT NULL CHECK(length(sealed_value) > 9),
    updated_unix_ms INTEGER NOT NULL CHECK(updated_unix_ms >= 0)
);

CREATE TABLE mcp_grants (
    blueprint_name TEXT NOT NULL CHECK(length(blueprint_name) > 0),
    server_name TEXT NOT NULL CHECK(length(server_name) > 0),
    login_id TEXT NOT NULL CHECK(length(login_id) > 0),
    kind TEXT NOT NULL CHECK(kind IN ('refresh', 'static')),
    sealed_token BLOB NOT NULL CHECK(length(sealed_token) > 9),
    client_id TEXT,
    token_endpoint TEXT,
    revision INTEGER NOT NULL CHECK(revision >= 0),
    updated_unix_ms INTEGER NOT NULL CHECK(updated_unix_ms >= 0),
    PRIMARY KEY(blueprint_name, server_name),
    CHECK((kind = 'refresh' AND token_endpoint IS NOT NULL AND length(token_endpoint) > 0)
       OR (kind = 'static' AND client_id IS NULL AND token_endpoint IS NULL))
);

CREATE TABLE mcp_grant_scopes (
    blueprint_name TEXT NOT NULL,
    server_name TEXT NOT NULL,
    position INTEGER NOT NULL CHECK(position >= 0),
    scope TEXT NOT NULL,
    PRIMARY KEY(blueprint_name, server_name, position),
    FOREIGN KEY(blueprint_name, server_name) REFERENCES mcp_grants(blueprint_name, server_name) ON DELETE CASCADE
);
