-- lumina migration 0017: work_items.checkpoint 0/1 write-time guard
-- (review R12 follow-up to migration 0016's sprint-lifecycle & worktree substrate).
--
-- ## Why
-- Migration 0016 added `work_items.checkpoint` as a bare nullable INTEGER with
-- NO column-level CHECK (SQLite cannot `ALTER TABLE ADD CONSTRAINT`). The
-- claim-time checkpoint-freeze barrier in `claim_next_task` keys STRICTLY on
-- `checkpoint = 1`, so any out-of-band value other than 0/1 (a stray `2` written
-- by raw SQL, a manual fix-up, or a future bug) would silently FAIL to freeze
-- the sprint — the sprint-wide barrier disabled for that task with no error. The
-- repo layer only ever writes 0/1 via `set_task_checkpoint`, but defense-in-depth
-- at the DB write boundary closes the gap.
--
-- ## The approach — validation triggers, NOT a table rebuild
-- SQLite cannot add a CHECK to an existing column in place. Rather than the
-- 12-step CREATE/INSERT/DROP/RENAME rebuild of the now-large, heavily
-- FK-referenced `work_items` table (the migration-0007 path, which predates the
-- 0010 / 0013 / 0016 column additions and would have to replicate every column,
-- index, and trigger exactly), this migration mirrors the table's EXISTING
-- write-time validation idiom — `trg_work_items_hierarchy_*` (0001) and
-- `trg_work_items_attributes_*` (0002) — with a BEFORE INSERT / BEFORE UPDATE
-- trigger pair that RAISE(ABORT)s when `checkpoint` is non-NULL and not in
-- (0, 1). NULL stays legal (the implicit default — "not a checkpoint task").
-- This is functionally equivalent to a column-level CHECK for every write path.
-- Forward-only; no down-migration.

CREATE TRIGGER trg_work_items_checkpoint_insert
BEFORE INSERT ON work_items
FOR EACH ROW
WHEN NEW.checkpoint IS NOT NULL AND NEW.checkpoint NOT IN (0, 1)
BEGIN
    SELECT RAISE(ABORT, 'invalid work_item checkpoint: must be NULL, 0, or 1');
END;

CREATE TRIGGER trg_work_items_checkpoint_update
BEFORE UPDATE ON work_items
FOR EACH ROW
WHEN NEW.checkpoint IS NOT NULL AND NEW.checkpoint NOT IN (0, 1)
BEGIN
    SELECT RAISE(ABORT, 'invalid work_item checkpoint: must be NULL, 0, or 1');
END;
