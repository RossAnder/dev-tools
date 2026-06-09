-- lumina migration 0016: sprint-lifecycle & worktree substrate (ADD-COLUMN + new tables, PURELY ADDITIVE).
--
-- Forward-only; no down-migration. Recovery if a later task fails: `git revert`
-- this file and recreate the gitignored dev DB (db::init / `sqlx migrate run`
-- rebuilds it from the embedded migration set). This migration adds two new
-- tables (`worktrees`, `task_commits`), three nullable columns (two on `sprints`,
-- one on `work_items`), four indexes, and one forward-only `UPDATE` backfill of
-- legacy sprint statuses — it touches no existing column, default, index, or
-- trigger and changes no existing query result (beyond renaming legacy `'open'`
-- sprint rows to `'active'`, see backfill below), so it breaks no current
-- consumer or test; a wipe-and-recreate is safe (the dev DB carries no live
-- sprint-lifecycle/worktree data).
--
-- ## Why ([ADR-0002](../../docs/adr/0002-sprint-execution-architecture.md) layer 2)
-- Make the Sprint a fully-tracked lifecycle entity and introduce a first-class
-- Worktree as the inter-sprint isolation + merge unit:
--   * `worktrees`                  — a worktree is OWNED BY EXACTLY ONE sprint
--                                    (`owning_sprint_id` is a UNIQUE FK → sprints,
--                                    1:1 with its owner). Its lifecycle status is
--                                    WHOLLY DERIVED from the owning sprint — there
--                                    is deliberately NO `worktrees.status` column;
--                                    `get_worktree` JOINs the owner for an
--                                    `effective_status`. The table carries
--                                    merge-AUDIT only (`merged_at`/`merge_ref`/
--                                    `outcome`). lumina is RECORD-ONLY: it NEVER
--                                    shells out to git.
--   * `sprints.worktree_id`        — which worktree a sprint RUNS IN. The owner
--                                    and every follow-up sprint share the same
--                                    `worktree_id`; the owner is the one row where
--                                    `worktrees.owning_sprint_id = sprint.id`.
--   * `sprints.predecessor_sprint_id` — nullable self-FK threading run-chaining
--                                    provenance: a follow-up fix sprint TARGETS a
--                                    predecessor's unmerged worktree (shares its
--                                    `worktree_id`) without owning it.
--   * `work_items.checkpoint`      — a nullable 0/1 flag marking a checkpoint
--                                    barrier task. RUNTIME-FREEZE only (ADR-0003):
--                                    `claim_next_task` freezes the whole sprint
--                                    while any checkpoint task is `in_progress`;
--                                    it is NOT auto-wired as a task→task dep.
--   * `task_commits`               — an explicit-task-id-list commit cross-
--                                    reference (pure audit): the committing lead
--                                    passes the task-ids a commit covers.
--
-- ## ADD-COLUMN-REFERENCES rule (mirrors 0011 lines 30-37 / 0013 lines 35-42)
-- `ALTER TABLE ... ADD COLUMN ... REFERENCES` requires a NULL default in SQLite
-- (it cannot add a NOT-NULL FK column without a non-NULL default, and an FK
-- default would have to reference an existing row). So `sprints.worktree_id`
-- (→ worktrees), `sprints.predecessor_sprint_id` (self-FK → sprints) and
-- `work_items.checkpoint` are all nullable with the implicit NULL default —
-- which is the intended semantics (a sprint runs in no worktree until one is
-- created; legacy/non-checkpoint tasks leave `checkpoint` NULL). SQLite's ALTER
-- TABLE adds exactly ONE column per statement, so the three new columns are
-- three separate `ALTER TABLE ... ADD COLUMN ...;` statements.
--
-- ## No CHECK on sprints.status (free TEXT, repo-enforced)
-- `sprints.status` is FREE TEXT with NO CHECK (migration 0011:53) and stays that
-- way: SQLite cannot `ALTER TABLE ... ADD CONSTRAINT`, so retrofitting a CHECK
-- would need the full table-rebuild dance this forward-only/purely-additive plan
-- deliberately avoids. The new typed lifecycle vocab
-- (`draft|ready|active|review|done|cancelled`) is enforced at the REPO layer
-- (typed `SprintStatus` + `set_sprint_status` legal-transition validation),
-- exactly mirroring `work_items.status` (also free TEXT + Rust-enforced). The new
-- `worktrees.outcome` column DOES carry a CHECK — it is a column on a NEW table
-- (created here, not ALTER-ed), so the CHECK is part of the CREATE TABLE and
-- needs no rebuild.
--
-- ## Backfill: 'open' -> 'active' (NOT 'open' -> 'draft')
-- Layer-1 only ever writes `'open'` for a sprint status, and `'open'` meant
-- "runnable" (the claim treated any non-terminal sprint as claimable). The
-- stricter layer-2 guard makes claim runnable ⟺ `status='active'`, so legacy
-- `'open'` rows are backfilled to `'active'` to PRESERVE their runnable
-- behaviour — backfilling to `'draft'` would silently make every pre-existing
-- sprint non-runnable. Verified: `'open'` is the only sprint status any
-- production path writes; `'closed'`/`'merged'` live only in the layer-1
-- `NON_RUNNABLE_SPRINT_STATUSES` const + a test fixture (never persisted by a
-- production mutator), so this single forward-only UPDATE leaves no live sprint
-- row holding an out-of-vocab status. The UPDATE runs LAST (after all DDL) and
-- is forward-only — it is NEVER an edit to migration 0011.
--
-- ## FK-safe statement order
-- Tables are created BEFORE anything that REFERENCES them: `worktrees` is created
-- FIRST (it REFERENCES the existing `sprints`), THEN `sprints.worktree_id`
-- (→ worktrees) can be added; `sprints.predecessor_sprint_id` (self-FK → sprints)
-- and `work_items.checkpoint` follow; `task_commits` (→ work_items + sprints,
-- both pre-existing) is created next; the four indexes (over the now-existing
-- columns) and the backfill UPDATE come last.
--
-- ## INTENT on FK delete actions
-- Every new FK below (worktrees.owning_sprint_id → sprints; sprints.worktree_id →
-- worktrees; sprints.predecessor_sprint_id → sprints; task_commits.task_id →
-- work_items; task_commits.sprint_id → sprints) is left at the SQLite default
-- ON DELETE NO ACTION — consistent with the store's soft-delete model (rows are
-- tombstoned via `deleted_at`, never hard-DELETEd), mirroring the 0011 note. If a
-- future hard-delete/purge path lands, these FKs become referential-integrity
-- blockers (FK errors) rather than silent cascades — review that intent before
-- adding any DELETE statements.

-- worktrees: the inter-sprint isolation + merge unit, owned by exactly one
-- sprint (UNIQUE FK), status WHOLLY DERIVED from the owner (no status column),
-- merge-AUDIT only. Created FIRST so `sprints.worktree_id` can reference it.
CREATE TABLE worktrees (
    id               TEXT PRIMARY KEY,                                            -- uuid v7
    owning_sprint_id TEXT NOT NULL UNIQUE REFERENCES sprints(id),                 -- 1:1 owner; UNIQUE so a sprint owns at most one worktree
    path             TEXT NOT NULL,                                               -- the checkout path (recorded; lumina never touches it)
    base_ref         TEXT,                                                        -- nullable: the ref the worktree branched from
    branch           TEXT,                                                        -- nullable: the worktree's branch name
    merged_at        TEXT,                                                        -- nullable: ISO-8601 stamp set on merge/rejection (audit)
    merge_ref        TEXT,                                                        -- nullable: the ref/sha the worktree merged into (audit)
    outcome          TEXT CHECK (outcome IS NULL OR outcome IN ('merged', 'rejected')), -- nullable terminal disposition (audit)
    created_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at       TEXT                                                         -- nullable: soft-delete tombstone (NULL = live)
);

ALTER TABLE sprints    ADD COLUMN worktree_id           TEXT REFERENCES worktrees(id); -- nullable (ADD-COLUMN-REFERENCES rule): the worktree this sprint runs in
ALTER TABLE sprints    ADD COLUMN predecessor_sprint_id TEXT REFERENCES sprints(id);   -- nullable self-FK (ADD-COLUMN-REFERENCES rule): run-chaining provenance
ALTER TABLE work_items ADD COLUMN checkpoint            INTEGER;                        -- nullable 0/1: checkpoint-barrier flag (NULL = not a checkpoint; runtime-freeze only)

-- task_commits: explicit-task-id-list commit cross-reference (pure audit). One
-- row per (commit, task) pair; `sprint_id` is the optional sprint the commit
-- landed under. REFERENCES the pre-existing work_items + sprints.
CREATE TABLE task_commits (
    id          TEXT PRIMARY KEY,                                                 -- uuid v7
    commit_sha  TEXT NOT NULL,                                                    -- the commit sha the task is covered by
    task_id     TEXT NOT NULL REFERENCES work_items(id),                          -- the covered task
    sprint_id   TEXT REFERENCES sprints(id),                                      -- nullable: the sprint the commit landed under
    recorded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Worktree-by-owner / sprints-in-worktree lookup: partial over the live
-- (non-NULL) `worktree_id` rows so the planner SEARCHes the share-set instead of
-- SCANning all of sprints. Mirrors the 0013 partial-index style.
CREATE INDEX idx_sprints_worktree ON sprints(worktree_id) WHERE worktree_id IS NOT NULL;
-- list_task_commits read paths: by task_id and by commit_sha.
CREATE INDEX idx_task_commits_task   ON task_commits(task_id);
CREATE INDEX idx_task_commits_commit ON task_commits(commit_sha);
-- Idempotency anchor: `record_task_commits` inserts `ON CONFLICT(commit_sha,
-- task_id) DO NOTHING`, so a re-record of the same (commit, task) pair collapses
-- on this UNIQUE index rather than duplicating an audit row.
CREATE UNIQUE INDEX ux_task_commits ON task_commits(commit_sha, task_id);

-- Backfill legacy 'open' sprints to 'active' (see "Backfill" header note):
-- 'open' meant runnable, and the layer-2 guard makes runnable ⟺ 'active', so
-- this PRESERVES the runnable behaviour. Forward-only; runs LAST.
UPDATE sprints SET status = 'active' WHERE status = 'open';
