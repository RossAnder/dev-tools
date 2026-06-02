-- lumina migration 0012: performance indexes (schema-only, PURELY ADDITIVE).
--
-- Forward-only; no down-migration. Recovery if a later task fails: `git revert`
-- this file and recreate the gitignored dev DB (db::init / `sqlx migrate run`
-- rebuilds it from the embedded migration set). This migration only adds /
-- replaces secondary indexes — it touches no table shape and changes no query
-- result, so it breaks no existing consumer or test. SQLite's planner picks
-- indexes by structure, not by name, so no code references these index names.
--
-- ## Why
-- Two hot read paths grow with the row count and benefit from covering /
-- partial indexes that satisfy both the filter and the sort with no extra step.

-- O8: covering index for the hot live-finding read path (list_findings /
-- get_story_finding_queue / the get_work_item_detail fold all filter
-- `work_item_id = ? AND superseded_by IS NULL` and sort `first_flagged DESC`).
-- Anchors the work_item_id bucket, the live filter, and the sort.
CREATE INDEX idx_findings_live ON findings(work_item_id, superseded_by, first_flagged DESC);

-- O9: partial index for the export drain's pending-events scan, which runs
-- `WHERE exported_at IS NULL ORDER BY created_at, id`. The partial index contains
-- ONLY pending rows (so it satisfies the predicate) AND is ordered by
-- (created_at, id) (so it satisfies the sort), keeping the drain O(pending) with
-- no sort step as `events` grows. It supersedes the old full index on exported_at.
DROP INDEX IF EXISTS idx_events_unexported;
CREATE INDEX idx_events_pending ON events(created_at, id) WHERE exported_at IS NULL;
