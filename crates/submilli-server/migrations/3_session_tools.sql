-- The application tools a harness declared when it opened the session (`@tools`),
-- as the JSON array it sent. Fixed for the session's life; NULL when none.
ALTER TABLE sessions ADD COLUMN tools_json TEXT CHECK (tools_json IS NULL OR json_valid(tools_json));
