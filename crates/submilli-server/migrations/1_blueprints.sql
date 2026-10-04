CREATE TABLE blueprint_revisions (
    name TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    yaml TEXT NOT NULL,
    PRIMARY KEY (name, revision)
);
CREATE TABLE blueprints (
    name TEXT PRIMARY KEY NOT NULL,
    current_revision INTEGER NOT NULL,
    FOREIGN KEY (name, current_revision) REFERENCES blueprint_revisions(name, revision)
);
