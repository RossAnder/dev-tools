-- lumina migration 0026: story-scoped plan epoch, open-question retire signal,
-- and a persisted task <-> research grounding edge (story-planning-round-5).
--
-- Schema foundation for round-5's planning-orchestrator rework:
--   1. A monotonic per-work_item rework "plan epoch" (NOT NULL DEFAULT 0 on
--      work_items; nullable on the five planning child tables so a record can be
--      stamped with the epoch it was authored under).
--   2. A "retire" liveness signal on open_questions (the rework path retires a
--      question without deleting it, preserving the audit trail).
--   3. A first-class task <-> research grounding edge (task_research_links) so a
--      task's research provenance survives as a queryable edge, not just prose.
--
-- ## Design constraints (mirroring 0003 / 0005 / 0023 / 0024)
--   * SQLite ALTER TABLE ADD COLUMN cannot carry a non-constant DEFAULT, a
--     PK/UNIQUE, or a retroactive CHECK; an FK/ON DELETE clause is legal ONLY
--     inside CREATE TABLE (never on ADD COLUMN). The NOT NULL add on work_items
--     is legal ONLY because it carries a constant DEFAULT 0; the child-table
--     adds are plain nullable INTEGER (no DEFAULT, no NOT NULL).
--   * `task_research_links` is a FRESH table, so full FK clauses with ON DELETE
--     CASCADE are legal (both sides die with their parent).
--   * `id` columns elsewhere are TEXT holding app-generated UUIDv7;
--     `created_at` defaults CURRENT_TIMESTAMP, consistent with 0003/0005/0023.
-- Forward-only; no down-migration. LF line endings (a CRLF migration risks an
-- sqlx checksum anomaly on a future renormalize — see 0001's known CRLF issue).

PRAGMA foreign_keys = ON;

-- ---------------------------------------------------------------------------
-- work_items: the rework plan epoch. NOT NULL DEFAULT 0 — legal as an ADD
-- COLUMN only because the constant DEFAULT backfills existing rows to epoch 0.
-- ---------------------------------------------------------------------------
ALTER TABLE work_items ADD COLUMN plan_epoch INTEGER NOT NULL DEFAULT 0;

-- ---------------------------------------------------------------------------
-- Planning child tables: nullable plan_epoch stamp (the epoch a record was
-- authored under). Nullable, NO DEFAULT, NO NOT NULL — plain ADD COLUMN.
-- ---------------------------------------------------------------------------
ALTER TABLE research_notes ADD COLUMN plan_epoch INTEGER;
ALTER TABLE risks ADD COLUMN plan_epoch INTEGER;
ALTER TABLE rejected_alternatives ADD COLUMN plan_epoch INTEGER;
ALTER TABLE open_questions ADD COLUMN plan_epoch INTEGER;
ALTER TABLE acceptance_criteria ADD COLUMN plan_epoch INTEGER;

-- ---------------------------------------------------------------------------
-- open_questions: the rework liveness signal. Nullable TEXT timestamp; NULL =
-- live, a non-NULL ISO-8601 value = retired by a rework pass (not deleted, so
-- the audit trail survives).
-- ---------------------------------------------------------------------------
ALTER TABLE open_questions ADD COLUMN retired_at TEXT;

-- ---------------------------------------------------------------------------
-- task_research_links: the persisted task <-> research grounding edge. One row
-- per (task, research_note); both FKs ON DELETE CASCADE (the edge is
-- meaningless without either endpoint). Composite PK enforces set membership.
-- ---------------------------------------------------------------------------
CREATE TABLE task_research_links (
    task_id          TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    research_note_id TEXT NOT NULL REFERENCES research_notes(id) ON DELETE CASCADE,
    created_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (task_id, research_note_id)
);
