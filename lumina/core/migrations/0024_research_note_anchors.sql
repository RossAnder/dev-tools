-- Forward-only: nullable TEXT column holding a JSON array of anchor strings
-- (each entry a "path:line" file anchor or an http(s) URL); NULL = no anchors.
-- Nullable, no DEFAULT, no CHECK -- matches the migration-0004 repo_links
-- forward-only ADD COLUMN precedent (SQLite ADD COLUMN nullable-TEXT rules).
ALTER TABLE research_notes ADD COLUMN anchors TEXT;
