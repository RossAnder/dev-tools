-- lumina migration 0028: scheduled_units (in-process tokio scheduler foundation)
-- + per-story work_items.drive_depth. PURELY ADDITIVE, forward-only.
--
-- ## Why
-- Focus 1C.3 introduces an in-process tokio scheduler that drives planning /
-- sprint-composition / merge work at STORY/SPRINT scale. It needs two things this
-- migration lays down (the dispatch-lease primitive + the scheduler loop that
-- consume them are the NEXT tasks):
--
--   1. `scheduled_units` — a DEDICATED durable claim/lease table for scheduled
--      units of work. A unit = (kind, work_item_id) with its own claim owner +
--      lease deadline. This is deliberately a SEPARATE table from `work_items`
--      and NOT an overload of the team-execution `work_items.assignee` /
--      `lease_expires_at` columns (migration 0013): those carry TASK-claim
--      semantics for the per-task agent work-queue, whereas a scheduled unit
--      claims a STORY/SPRINT-scale driver job. Mixing the two leases on one row
--      would conflate "an agent is editing this task" with "the scheduler is
--      driving this story", so they stay on distinct rows.
--
--   2. `work_items.drive_depth` — a nullable per-STORY column the grill sets to
--      record how far the scheduler should autonomously drive a story:
--      'plan-only' | 'compose-sprint' | 'drive-to-merge'. NULL = unset (no
--      autonomous drive decision recorded). Mirrors the nullable-CHECK column
--      idiom of `work_items.lane` (0013) / `tier` (0006) / `task_kind` (0005):
--      the vocabulary is enforced both by the DB CHECK here AND by the typed
--      `DriveDepth` enum in the repo/wire layer.
--
-- ## ADD-COLUMN-CHECK rule (mirrors 0013's `lane`)
-- `ALTER TABLE ... ADD COLUMN` in SQLite requires the column's default to satisfy
-- any CHECK. `drive_depth` is nullable with the implicit NULL default, and the
-- CHECK is written `drive_depth IS NULL OR drive_depth IN (...)` so every
-- pre-existing row (all taking the NULL default) passes. SQLite cannot
-- `ALTER TABLE ... DROP COLUMN` a CHECK-constrained column cleanly, so reverting
-- means a NEW forward migration that neutralises the column, never a down-migration.
--
-- ## CREATE TABLE FK clauses are legal here
-- `scheduled_units` is a FRESH table, so a full `REFERENCES work_items(id)` FK
-- clause is legal (the ADD-COLUMN-REFERENCES NULL-default restriction applies only
-- to column-add, not CREATE TABLE — as in 0020's `task_files`).
--
-- ## Design constraints (mirroring 0001-0027)
--   * `id` is TEXT holding an app-generated UUIDv7 (generation in the repo layer).
--   * `created_at` / `updated_at` default CURRENT_TIMESTAMP (the 0001/0005/0020 idiom).
--   * `status` is FREE TEXT with a sensible 'pending' default and NO rigid CHECK,
--     mirroring `work_items.status` / `sprints.status` (repo-validated vocab — a
--     CHECK would block future scheduler states without a table rebuild).
--   * `kind` IS CHECK-constrained — it is a closed dispatch vocabulary the
--     scheduler switches on, so a stray value must fail loudly at the DB layer
--     (mirroring the `lane`/`tier` CHECK columns).
-- Forward-only; no down-migration. LF line endings (a CRLF migration risks an
-- sqlx checksum anomaly on a future renormalize -- see 0001's known CRLF issue).

PRAGMA foreign_keys = ON;

-- ---------------------------------------------------------------------------
-- work_items.drive_depth: per-STORY autonomous-drive depth (the grill sets it).
--   NULL                = unset (no drive decision recorded).
--   'plan-only'         = stop after planning.
--   'compose-sprint'    = plan + compose a sprint, then stop.
--   'drive-to-merge'    = plan + compose + drive the sprint through to merge.
-- ---------------------------------------------------------------------------
ALTER TABLE work_items ADD COLUMN drive_depth TEXT
    CHECK (drive_depth IS NULL OR drive_depth IN ('plan-only', 'compose-sprint', 'drive-to-merge'));

-- ---------------------------------------------------------------------------
-- scheduled_units: one durable scheduler claim/lease per (kind, work_item).
--   kind            — the scheduler dispatch kind (CHECK-constrained closed set).
--   work_item_id    — FK to work_items(id): the story/sprint work-item the unit drives.
--   status          — free-text lifecycle (repo-validated; defaults 'pending').
--   assignee        — the scheduler worker id holding the lease; NULL = unclaimed.
--   lease_expires_at— ISO-8601 lease deadline; a past value is reclaimable
--                     (lazy reclaim by the claim sweep, mirroring 0013 — no reaper).
--   plan_epoch      — the work-item's plan_epoch captured at dispatch time.
-- ---------------------------------------------------------------------------
CREATE TABLE scheduled_units (
    id               TEXT NOT NULL PRIMARY KEY,
    kind             TEXT NOT NULL
                         CHECK (kind IN ('build_story', 'build_tasks', 'compose_sprint', 'drive')),
    work_item_id     TEXT NOT NULL REFERENCES work_items(id),
    status           TEXT NOT NULL DEFAULT 'pending',
    assignee         TEXT,
    lease_expires_at TEXT,
    plan_epoch       INTEGER NOT NULL DEFAULT 0,
    created_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- At most one scheduled unit per (kind, work_item): a unit must not be
-- double-created for the same driver job. A UNIQUE index (no NULLs in either
-- column — both NOT NULL — so the plain UNIQUE dedupes correctly, unlike the
-- COALESCE expression indexes that 0019/0020 needed for nullable columns).
CREATE UNIQUE INDEX idx_scheduled_units_kind_item
    ON scheduled_units(kind, work_item_id);

-- Claim hot path: the scheduler selects the next ready unit by status. Mirrors
-- the 0013 `idx_work_items_claim` style (a plain status index here, since the
-- candidate predicate is status-led).
CREATE INDEX idx_scheduled_units_status
    ON scheduled_units(status);

-- Lazy reclaim: the claim's first statement sweeps expired leases
-- (`WHERE assignee IS NOT NULL AND lease_expires_at < :now`). A partial index
-- over only the leased rows keeps that sweep O(leased) — mirrors 0013's
-- `idx_work_items_lease`.
CREATE INDEX idx_scheduled_units_lease
    ON scheduled_units(lease_expires_at) WHERE assignee IS NOT NULL;
