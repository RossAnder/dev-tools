-- lumina migration 0022: run-level open-question dedup key (ADD-COLUMN +
-- partial UNIQUE index, PURELY ADDITIVE).
--
-- Forward-only; no down-migration. Recovery if a later task fails: `git revert`
-- this file and recreate the gitignored dev DB (db::init / `sqlx migrate run`
-- rebuilds it from the embedded migration set). This migration adds ONE nullable
-- `open_questions` column plus a fresh partial UNIQUE index — it touches no
-- existing column, default, index, or trigger and changes no existing query
-- result (every `open_questions` SELECT names its columns explicitly, never
-- `SELECT *`), so it breaks no current consumer or test; a wipe-and-recreate is
-- safe.
--
-- ## Why (focus 1C.1 durable async comms — research notes seq22 + seq29)
-- `open_questions` is STORY-scoped (only a `story_id` FK; NO `sprint_id`/`run_id`
-- column). The orchestrator's "raise a RUN-level decision" use case has no
-- first-class home, so the accepted decision (a dedicated run-scoped owner is
-- DEFERRED) is to REUSE a story-scoped `open_questions` row tagged to the run.
-- The gap this closes (seq29): two non-PTY teammates that hit the SAME run-level
-- decision would each call the create path and produce DUPLICATE questions. A
-- caller-supplied DEDUP KEY (the "run tag" lives in the key) lets the second
-- create collapse onto the first instead of duplicating.
--
-- ## Column (ADD-COLUMN only — no table rebuild)
--   * `run_dedup_key` — nullable TEXT, NO DEFAULT, NO CHECK. The caller-supplied
--     idempotency key for a run-level question (typically `<run_id>:<slug>` or a
--     content hash). NULL = a plain story-scoped question (the existing
--     `add_open_question` / `escalate_decision_and_park_task` paths leave it NULL
--     and are entirely unaffected — they neither set nor read it). Nullable-with-
--     no-default is the only shape SQLite's `ALTER TABLE ADD COLUMN` accepts for a
--     forward-only additive column whose existing rows have no value (precedent:
--     `open_questions.resume_epoch` in 0021, `findings.repo_id` in 0004).
--
-- ## Idempotency index
-- A PARTIAL UNIQUE index over `(story_id, run_dedup_key)` restricted to LIVE
-- (status='open') keyed rows. Scoping to `status='open'` (not all rows) means a
-- key is only "taken" while its question is unresolved — once a run-level
-- question is answered/cancelled, the SAME key may be re-raised for a fresh
-- decision (the run repeats). Scoping to `run_dedup_key IS NOT NULL` leaves the
-- NULL-key plain-question rows entirely unconstrained (they may repeat freely).
-- A fresh CREATE INDEX with a partial predicate is fully supported by SQLite
-- (unlike ADD CONSTRAINT). The repo-layer create-path pre-check (inside one
-- BEGIN IMMEDIATE tx, which serialises writers at begin-time) is the primary
-- collapse mechanism; this index is the belt-and-braces backstop against a
-- record-layer race.
--
-- ## ROLLBACK RECIPE (forward-only — no down-migration, no DROP COLUMN)
-- To neutralise: `DROP INDEX idx_open_questions_run_dedup; UPDATE open_questions
-- SET run_dedup_key = NULL;`. The column stays (SQLite `DROP COLUMN` is avoided
-- per the forward-only discipline, mirrors 0013/0015/0020/0021); any true revert
-- is a NEW forward migration that neutralises it, never a down-migration.

ALTER TABLE open_questions ADD COLUMN run_dedup_key TEXT;                         -- nullable, NO DEFAULT/CHECK: caller-supplied run-level idempotency key; NULL = a plain story-scoped question (unconstrained)

CREATE UNIQUE INDEX idx_open_questions_run_dedup
    ON open_questions (story_id, run_dedup_key)
    WHERE run_dedup_key IS NOT NULL AND status = 'open';                          -- at most one OPEN run-level question per (story, key); a resolved/cancelled question frees the key for re-raise
