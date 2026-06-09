-- lumina migration 0013: team-execution queue (schema-only, PURELY ADDITIVE).
--
-- Forward-only; no down-migration. Recovery if a later task fails: `git revert`
-- this file and recreate the gitignored dev DB (db::init / `sqlx migrate run`
-- rebuilds it from the embedded migration set). This migration only adds four
-- nullable `work_items` columns + two secondary indexes — it touches no
-- existing column, default, or trigger and changes no existing query result,
-- so it breaks no current consumer or test; a wipe-and-recreate is safe
-- (the dev DB carries no live team-execution data).
--
-- NOTE on rollback under SQLite: the new `lane` CHECK column and the
-- `reviews_work_item_id` self-FK column cannot be cleanly `DROP COLUMN`-ed
-- (SQLite's ALTER TABLE DROP COLUMN refuses a column referenced by a CHECK or
-- an index, and a retroactive FK/CHECK removal needs the full table-rebuild
-- dance). So reverting 0013 means a NEW forward migration that neutralises the
-- columns — never a down-migration or a manual DROP.
--
-- ## Why
-- The team-execution work-queue (eventual-leaping-metcalfe plan) needs a small
-- amount of per-task lease/lane state so a team of agents can atomically claim,
-- lease, and review tasks out of a shared SQLite queue:
--   * `assignee`             — the agent id currently holding the lease on a
--                              task (NULL = unclaimed).
--   * `lease_expires_at`     — ISO-8601 lease deadline; a past value is reclaimed
--                              by the claim's lazy-reclaim sweep (no background
--                              reaper).
--   * `lane`                 — which queue a team-managed task sits in
--                              ('implement' | 'review'); NULL = not team-managed,
--                              so legacy tasks stay invisible to the claim
--                              (back-compat). "review" is a LANE, never a tier —
--                              `tier` (0006) stays CHECK(NULL|lite|deep).
--   * `reviews_work_item_id` — review task → the implementation task it covers
--                              (reviewer target + audit back-link).
--
-- ## ADD-COLUMN-REFERENCES rule (mirrors 0011 lines 30-37)
-- `ALTER TABLE ... ADD COLUMN ... REFERENCES` requires a NULL default in SQLite
-- (it cannot add a NOT-NULL FK column without a non-NULL default, and an FK
-- default would have to reference an existing row). `reviews_work_item_id` is
-- therefore nullable with the implicit NULL default — which is the intended
-- semantics (only review tasks carry a back-link). The CHECK column `lane` is
-- written `lane IS NULL OR lane IN (...)` (mirroring the 0006 `tier` idiom) so
-- it passes for every pre-existing row, all of which take the NULL default.
--
-- ## One column per ALTER
-- SQLite's ALTER TABLE adds exactly ONE column per statement, so the four new
-- columns are four separate `ALTER TABLE work_items ADD COLUMN ...;` statements.

ALTER TABLE work_items ADD COLUMN assignee             TEXT;                        -- nullable: lease holder (NULL = unclaimed)
ALTER TABLE work_items ADD COLUMN lease_expires_at     TEXT;                        -- nullable: ISO-8601 lease deadline
ALTER TABLE work_items ADD COLUMN lane                 TEXT
    CHECK (lane IS NULL OR lane IN ('implement', 'review'));                        -- NULL = not team-managed (invisible to claim)
ALTER TABLE work_items ADD COLUMN reviews_work_item_id TEXT REFERENCES work_items(id); -- nullable (ADD-COLUMN-REFERENCES rule)

-- Claim hot path: the team claim selects the next ready task by
-- `(lane, tier, status)` over LIVE rows. A partial index keyed on those three
-- columns and restricted to live rows (`deleted_at IS NULL`) lets the planner
-- SEARCH the candidate set instead of SCANning all of work_items. Mirrors the
-- 0012 partial-index style.
CREATE INDEX idx_work_items_claim ON work_items(lane, tier, status) WHERE deleted_at IS NULL;

-- Lazy reclaim: the claim's first statement sweeps expired leases
-- (`WHERE assignee IS NOT NULL AND lease_expires_at < :now`). A partial index
-- over only the leased rows, ordered by `lease_expires_at`, keeps that sweep
-- O(leased) as work_items grows.
CREATE INDEX idx_work_items_lease ON work_items(lease_expires_at) WHERE assignee IS NOT NULL;
