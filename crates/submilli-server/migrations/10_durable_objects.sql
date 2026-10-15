-- Pending reservations protect both partial staging and unbound publications.
-- Tombstones remain after collection; an identity is never reused.
CREATE TABLE durable_object_store (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    identity TEXT NOT NULL
);

CREATE TABLE durable_objects (
    object_id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL,
    request_identity BLOB NOT NULL CHECK(length(request_identity)=32),
    maximum_bytes INTEGER NOT NULL CHECK(maximum_bytes>=0),
    reserved_bytes INTEGER NOT NULL CHECK(reserved_bytes>=0),
    publication TEXT NOT NULL CHECK(publication IN ('pending','bound','abandoned','retired')),
    collection TEXT NOT NULL CHECK(collection IN ('available','deleting','gone')),
    content BLOB,
    retention BLOB NOT NULL,
    revision INTEGER NOT NULL CHECK(revision>=0),
    CHECK((publication IN ('bound','retired')) = (content IS NOT NULL)),
    CHECK(collection='available' OR publication IN ('abandoned','retired'))
);
CREATE INDEX durable_objects_by_owner ON durable_objects(owner_id,collection);
CREATE INDEX durable_objects_for_recovery ON durable_objects(publication,collection,object_id);
