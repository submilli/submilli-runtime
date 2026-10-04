CREATE TABLE IF NOT EXISTS server_metadata (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    store_id TEXT NOT NULL,
    startup_generation TEXT NOT NULL
);
INSERT INTO server_metadata (singleton, store_id, startup_generation)
SELECT 1, lower(hex(randomblob(16))), '00000000-0000-0000-0000-000000000000'
WHERE NOT EXISTS (SELECT 1 FROM server_metadata);
DROP TABLE IF EXISTS schema_migrations;
-- Prevent a pre-SQLx executable from opening this database.
PRAGMA user_version = 2;
