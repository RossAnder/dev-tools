-- lumina migration 0025: neutralise `work_items.reviews_work_item_id` and
-- reconcile in-flight OLD-MODEL separate review tasks (story 1B-F9).
--
-- Forward-only; no down-migration. Recovery if a later task fails: `git revert`
-- this file and recreate the gitignored dev DB (`db::init` / `sqlx migrate run`
-- rebuilds it from the embedded migration set).
--
-- ## Why
-- The migration-0013 team-execution model spawned a SEPARATE `lane='review'`
-- work item on every `complete_task` of an implement-lane task, back-linked to
-- the implementation task via `work_items.reviews_work_item_id` and depending on
-- it through a `task_dependencies(review -> impl, 'sequence')` edge. Story 1B-F9
-- replaces that separate-task model with review-as-a-LANE-STATE on the impl task
-- itself, so NOTHING populates `reviews_work_item_id` after this change (the
-- `complete_task` cascade and its idempotency probe are retired in the
-- accompanying core rewrite). This migration closes the data half of that switch.
--
-- ## (a) `reviews_work_item_id` is now DEAD — left in place, no longer populated
-- SQLite cannot `DROP COLUMN` a column that is referenced by an index or carries
-- an FK self-reference, and a retroactive FK removal needs the full 12-step
-- table-rebuild dance (the migration-0007 path, which predates the 0010/0013/0016
-- column additions). So — mirroring the 0013 header's own "reverting means a NEW
-- forward migration that neutralises the columns, never a DROP" guidance — the
-- column STAYS, but is from here on WRITE-DEAD: no code path sets it, and this
-- file does not read it except to find the in-flight rows it must reconcile.
-- This is a NO-`DROP COLUMN`, forward-only neutralisation.
--
-- ## (b) Reconcile in-flight OLD-MODEL review rows so none orphan their sprint
-- A separate review task that was spawned before this change and is still
-- non-terminal would, under the new model, never be claimed or completed by
-- anything (the cascade that used to drive it is gone) — it would hang as a
-- non-terminal orphan that `get_sprint_quiescence` counts as not-done, so its
-- sprint can never reach `done`. We cancel exactly those rows: a task that is
-- `lane='review'` AND carries a `reviews_work_item_id` back-link (the unambiguous
-- old-model fingerprint) AND is not already terminal. Cancelling (not completing)
-- preserves the audit trail that the review never actually ran.
UPDATE work_items
SET status = 'cancelled',
    updated_at = CURRENT_TIMESTAMP
WHERE lane = 'review'
  AND reviews_work_item_id IS NOT NULL
  AND status NOT IN ('done', 'cancelled')
  AND deleted_at IS NULL;

-- Neutralise every `task_dependencies` edge that touches an old-model review row
-- (in EITHER direction): the review task's outgoing `review -> impl` 'sequence'
-- edge is now inert, and — defensively — any edge where a still-live task
-- `depends_on` one of these now-cancelled review rows would otherwise leave that
-- dependent blocked forever on a task that can never complete. Deleting the
-- edges removes both hazards; the `task_dependencies` join table is pure
-- prerequisite wiring (no event, no export), so a forward-only DELETE is safe.
DELETE FROM task_dependencies
WHERE task_id IN (
    SELECT id FROM work_items
    WHERE lane = 'review' AND reviews_work_item_id IS NOT NULL
)
   OR depends_on_id IN (
    SELECT id FROM work_items
    WHERE lane = 'review' AND reviews_work_item_id IS NOT NULL
);
