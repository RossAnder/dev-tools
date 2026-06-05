-- lumina migration 0015: harness session corpus (ADD-COLUMN + new table, PURELY ADDITIVE).
--
-- Forward-only; no down-migration. Recovery if a later task fails: `git revert`
-- this file and recreate the gitignored dev DB (db::init / `sqlx migrate run`
-- rebuilds it from the embedded migration set). This migration adds three
-- nullable-or-defaulted `pty_sessions` columns + one new `session_records`
-- table — it touches no existing column, default, index, or trigger and
-- changes no existing query result, so it breaks no current consumer or test;
-- a wipe-and-recreate is safe (the dev DB carries no live corpus data).
--
-- ## Why (ADR-0004 layer 2)
-- Capture every harness-controlled `claude` session — terminal (via a
-- SessionEnd http-hook) and SPA-spawned (existing live-tail) — into a durable,
-- LOSSLESS, cross-project corpus: one `session_records` row per ingested JSONL
-- line, stored VERBATIM in `raw`. `pty_messages` stays the derived render-view;
-- this table is the lossless-at-rest substrate. Sessions are export-INERT —
-- they never join the +1 work_items / +1 events invariant (one coarse
-- record_inert_event(tx, "session", …) per ingest). Redaction is egress-only
-- (layer 3); nothing is redacted at rest here.
--
-- ## pty_sessions correlation columns (ADD-COLUMN only — no table rebuild)
-- SQLite's ALTER TABLE adds exactly ONE column per statement, so the three new
-- columns are three separate `ALTER TABLE pty_sessions ADD COLUMN ...;`
-- statements:
--   * `source`    — provenance of the row. `NOT NULL DEFAULT 'spawned'` is
--                   legal under ALTER ADD COLUMN BECAUSE the DEFAULT backfills
--                   every PRE-EXISTING row (which ARE spawned) to 'spawned';
--                   the CHECK constrains it to ('spawned','ingested'). Ingested
--                   corpus rows are inserted with source='ingested'.
--   * `sprint_id` — nullable, NO foreign key: the sprint a session belongs to,
--                   harvested from the transcript's mcp__lumina__* records. A
--                   harvested sprint id is a `sprints.id` (migration 0011), NOT
--                   a work_items.id — and even a hard FK against sprints(id)
--                   would abort the LOSSLESS ingest on a deleted/cross-instance
--                   sprint, so this is kept FK-free: a best-effort correlation
--                   hint (mirroring agent_id), with full multi-sprint detail
--                   preserved in session_records per the plan's Q4 last-wins
--                   scalar + lossless-detail design.
--   * `agent_id`  — plain nullable TEXT (agents are NOT work_items, so this is
--                   NOT an FK); the agent that ran the session, also harvested.
--
-- NOTE — the `pty_sessions_project_kind_check_insert` / `_update` triggers
-- (migration 0008:24/34) fire on `NEW.project_id` only (INSERT-time, and
-- UPDATE OF project_id). They are UNAFFECTED by these three columns: none of
-- `source`/`sprint_id`/`agent_id` is `project_id`, so an ingested INSERT still
-- RAISE(ABORT)s the txn iff its `project_id` is not a live kind='project' row,
-- exactly as before. The new columns add no trigger of their own.
--
-- ## session_records — lossless verbatim corpus (one row per JSONL line)
-- `session_id … REFERENCES pty_sessions(id) ON DELETE RESTRICT` — RESTRICT is
-- chosen deliberately to PROTECT the lossless-at-rest guarantee: today deletes
-- are SOFT (`delete_pty_session` tombstones the row at `pty.rs:381`, never
-- `DELETE`s it), so no referential action ever fires and the corpus is
-- keep-forever by construction. RESTRICT (rather than CASCADE) means a FUTURE
-- hard `DELETE FROM pty_sessions` that still has corpus rows will FAIL LOUDLY
-- with a foreign-key violation instead of silently cascading-away the corpus —
-- a future hard-prune knob must therefore explicitly delete `session_records`
-- first (or otherwise reconcile the lossless-at-rest guarantee) before it can
-- remove a `pty_sessions` row. `id`/`created_at` follow the repo convention: `id` is
-- a uuidv7 TEXT PK, `created_at` an ISO-8601 TEXT timestamp (mirrors
-- pty_messages 0008:44-48). `UNIQUE(session_id, dedup_key)` makes ingest
-- idempotent — a re-harvest of the same line collapses on the dedup_key.
--
-- ## ROLLBACK RECIPE (forward-only — no down-migration, no DROP COLUMN)
-- The corpus is observational/inert, so recovery does NOT require a table
-- rebuild. To neutralise the data effects of this migration:
--   DELETE FROM session_records;
--   UPDATE pty_sessions SET sprint_id = NULL, agent_id = NULL WHERE source = 'ingested';
-- The schema COLUMNS stay: SQLite's `DROP COLUMN` cannot remove the `source`
-- CHECK without the full table-rebuild dance this plan deliberately avoids, and
-- per the forward-only discipline (mirrors 0013/0014) any true revert is a NEW
-- forward migration that neutralises the columns — never a down-migration or a
-- manual DROP.

ALTER TABLE pty_sessions ADD COLUMN source TEXT NOT NULL DEFAULT 'spawned'
    CHECK (source IN ('spawned', 'ingested'));            -- provenance; DEFAULT backfills existing rows to 'spawned'
ALTER TABLE pty_sessions ADD COLUMN sprint_id TEXT;                           -- nullable, NO FK: harvested correlation hint (a sprints.id, NOT a work_items.id); FK-free so a deleted/cross-instance sprint never aborts the lossless ingest (mirrors agent_id)
ALTER TABLE pty_sessions ADD COLUMN agent_id  TEXT;                           -- nullable: harvested agent (plain TEXT — agents aren't work_items)

CREATE TABLE session_records (
    id            TEXT PRIMARY KEY,                                           -- uuid v7
    session_id    TEXT NOT NULL REFERENCES pty_sessions(id) ON DELETE RESTRICT, -- RESTRICT: deletes are soft today (pty.rs:381); a future hard DELETE FROM pty_sessions fails loudly rather than cascading-away the lossless corpus
    line_ordinal  INTEGER NOT NULL,                                           -- 1-based position among NON-EMPTY lines (T4 contract; identical in live-tail and ingest)
    record_type   TEXT,                                                       -- nullable: JSONL record "type" (user|assistant|system|…), NULL if absent/unparsable
    record_uuid   TEXT,                                                       -- nullable: the record's own "uuid"
    parent_uuid   TEXT,                                                       -- nullable: the record's "parentUuid"
    ts            TEXT,                                                        -- nullable: the record's "timestamp"
    is_sidechain  INTEGER NOT NULL DEFAULT 0,                                 -- 0/1: the record's "isSidechain" flag
    raw           TEXT NOT NULL,                                              -- VERBATIM JSONL line (lossless-at-rest)
    dedup_key     TEXT NOT NULL,                                              -- content-derived idempotency key for re-harvest collapse
    created_at    TEXT NOT NULL,                                              -- ISO-8601 ingest timestamp
    UNIQUE (session_id, dedup_key)
);

-- Replay order: read a session's records in JSONL order.
CREATE INDEX idx_session_records_ordinal ON session_records(session_id, line_ordinal);
-- By-type lookup: (session_id, record_type). NOTE: currently UNUSED by the
-- harvest path — `harvest_correlation` scans the parsed Vec in Rust and never
-- queries session_records by record_type. Kept (not dropped) as a reservation
-- for a future egress/replay by-type query consumer; re-adding it later would be
-- migration churn. Do not assume harvest depends on this index.
CREATE INDEX idx_session_records_type ON session_records(session_id, record_type);
-- Cross-record correlation (parent/child threading) keyed on the record's own uuid;
-- partial — only rows that carry a uuid participate.
CREATE INDEX idx_session_records_uuid ON session_records(record_uuid) WHERE record_uuid IS NOT NULL;
