ALTER TABLE durable_outcomes ADD COLUMN storage_kind INTEGER NOT NULL DEFAULT 0 CHECK (storage_kind IN (0, 1));
