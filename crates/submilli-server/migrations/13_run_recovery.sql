-- Terminal runs retain their result but no longer need replay resources.
ALTER TABLE runs ADD COLUMN resources_retired INTEGER NOT NULL DEFAULT 0 CHECK(resources_retired IN (0,1));
CREATE INDEX runs_by_recovery ON runs(recovery_mode,resources_retired,run_id);
CREATE INDEX runs_by_session_state ON runs(session_id,state,admission_order);
