-- lumina migration 0011: runs/sprints/findings-queue (schema-only).
--
-- Forward-only; no down-migration. Recovery if a later task fails: `git revert`
-- this file and recreate the gitignored dev DB (db::init / `sqlx migrate run`
-- rebuilds it from the embedded migration set). This migration is PURELY
-- ADDITIVE — new tables, nullable FK columns, one partial index — so it breaks
-- no existing consumer or test; a wipe-and-recreate is safe (no live data).
--
-- ## Why
-- The review/optimise findings-queue domain needs first-class persistence for
-- the run → triage → spawn lifecycle that was previously implicit:
--   * `runs`             — one review or optimise pass over a sprint or story;
--                          status walks open → triaged → closed.
--   * `sprints`          — persisted sprint groupings (previously ephemeral);
--                          `sprint_tasks` is the sprint↔task membership link.
--   * `finding_decisions`— an append-only audit of every triage disposition a
--                          finding receives (spawn_task / spawn_story / defer /
--                          dismiss / resolve), optionally naming the spawned
--                          work item and who decided.
--   * `findings.run_id`  — the run a finding was raised under (nullable FK;
--                          legacy findings predate runs).
--   * `findings.triage_state` — the finding's place in the queue ('pending'
--                          until triaged).
--   * `work_items.spawned_from_finding_id` — provenance back-link from a work
--                          item spawned by a triage decision to its finding.
--   * `ux_findings_dedup`— a per-(work_item, dedup_id) partial UNIQUE index
--                          over LIVE findings (dedup_id present, not superseded)
--                          so a re-run cannot double-insert the same finding.
--
-- ## ADD-COLUMN-REFERENCES rule
-- `ALTER TABLE ... ADD COLUMN ... REFERENCES` requires a NULL default in SQLite
-- (it cannot add a NOT-NULL FK column without a non-NULL default, and an FK
-- default would have to reference an existing row). The three new FK columns
-- (`findings.run_id`, `work_items.spawned_from_finding_id`) are therefore
-- nullable, which is the intended semantics. `findings.triage_state` is a plain
-- TEXT column with a constant string default ('pending'), which ADD COLUMN does
-- permit.
--
-- ## FK-safe statement order
-- Tables are created BEFORE anything that REFERENCES them: `runs` and `sprints`
-- precede `sprint_tasks` (→ sprints + work_items), `finding_decisions`
-- (→ findings + work_items), `findings.run_id` (→ runs) and
-- `work_items.spawned_from_finding_id` (→ findings). The partial dedup index is
-- created last (it depends only on the existing `findings` columns).

-- runs: one review/optimise pass over a sprint or story.
CREATE TABLE runs (id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK (kind IN ('review','optimise')),
    target_id TEXT NOT NULL, target_kind TEXT NOT NULL CHECK (target_kind IN ('sprint','story')),
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','triaged','closed')),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);

-- sprints: persisted sprint groupings (previously ephemeral).
CREATE TABLE sprints (id TEXT PRIMARY KEY, title TEXT, status TEXT NOT NULL DEFAULT 'open',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);

-- INTENT: every FK below (sprint_tasks→sprints/work_items, finding_decisions→
-- findings/work_items, findings.run_id→runs, work_items.spawned_from_finding_id→
-- findings) is left at the SQLite default ON DELETE NO ACTION. This is deliberate
-- under the store's soft-delete model: rows are tombstoned/superseded, never hard
-- DELETEd, so the delete path is never exercised (consistent with the supersession
-- NO ACTION note on `supersede_finding` in repo.rs). If a future hard-delete/purge
-- path lands, these FKs become referential-integrity blockers (FK errors) rather
-- than silent cascades — review that intent before adding any DELETE statements.

-- sprint_tasks: sprint↔task membership link.
CREATE TABLE sprint_tasks (sprint_id TEXT NOT NULL REFERENCES sprints(id),
    task_id TEXT NOT NULL REFERENCES work_items(id), PRIMARY KEY (sprint_id, task_id));

-- finding_decisions: append-only triage-disposition audit per finding.
CREATE TABLE finding_decisions (id TEXT PRIMARY KEY, finding_id TEXT NOT NULL REFERENCES findings(id),
    decision TEXT NOT NULL CHECK (decision IN ('spawn_task','spawn_story','defer','dismiss','resolve')),
    spawned_work_item_id TEXT REFERENCES work_items(id), decided_by TEXT,
    decided_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);

ALTER TABLE findings   ADD COLUMN run_id       TEXT REFERENCES runs(id);   -- nullable (ADD-COLUMN-REFERENCES rule)
ALTER TABLE findings   ADD COLUMN triage_state TEXT DEFAULT 'pending';     -- constant default legal
ALTER TABLE work_items ADD COLUMN spawned_from_finding_id TEXT REFERENCES findings(id);

CREATE UNIQUE INDEX ux_findings_dedup ON findings(work_item_id, dedup_id)
    WHERE dedup_id IS NOT NULL AND superseded_by IS NULL;
