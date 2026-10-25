-- Existing content remains a sealed HistoryEntry. Only kind 1 stores a sealed reference.
ALTER TABLE durable_outcomes ADD COLUMN storage_kind INTEGER NOT NULL DEFAULT 0 CHECK(storage_kind IN (0,1));
CREATE TABLE history_object_store (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    identity TEXT NOT NULL,
    pending_directory_sync TEXT NOT NULL DEFAULT '[]'
);
CREATE TABLE history_objects (
    object_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(run_id),
    sequence INTEGER NOT NULL CHECK(sequence>=0),
    state TEXT NOT NULL CHECK(state IN ('pending','bound','deleting'))
);
CREATE INDEX history_objects_run ON history_objects(run_id);
CREATE INDEX history_objects_collection ON history_objects(state,object_id);
