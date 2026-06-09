-- Requires SQLite ≥3.38 (json1 built-in)
--
-- lumina migration 0005: round-2 planning schema (rejected alternatives,
-- risks, task dependencies, task_kind discriminator).
--
-- Additive / non-destructive over 0001-0004. Four classes of change:
--   1. New `rejected_alternatives` child table — per-work-item record of options
--      considered and discarded during planning, with a confidence grade and a
--      self-FK supersession chain. Mirrors the 0003 `research_notes` idiom.
--   2. New `risks` child table — per-work-item risk register with a CHECK-
--      constrained severity, free-text mitigation, and a self-FK supersession
--      chain. Mirrors the 0003 child-table idiom; severity is CHECK-enforced
--      (unlike research_notes.confidence) because risks gate sprint composition
--      and need a closed enum at the DB layer.
--   3. New `task_dependencies` join table — directed edges between tasks
--      (data | sequence | …), with a row-level CHECK against self-loops and a
--      BEFORE INSERT trigger that asserts both endpoints reference a
--      work_items row where kind='task'. Mirrors the 0004 `repo_links`
--      kind-check trigger shape (the column CHECK cannot subquery sibling
--      rows, so a trigger is the authoritative guard).
--   4. New nullable `work_items.task_kind` column — a task-scope discriminator
--      (foundation | vertical-slice | pattern-replacement | polish) used by
--      the sprint composer. Nullable, CHECK accepts NULL OR the enum literals.
--
-- Design constraints (mirroring 0001-0004):
--   * SQL kept ANSI-ish for a later Postgres port; the `task_dependencies`
--     kind-check trigger is the one intentional SQLite-specific construct
--     (a column CHECK cannot subquery sibling rows).
--   * All `id` columns are TEXT holding an app-generated UUIDv7; generation
--     lives in the repo layer.
--   * `created_at` uses CURRENT_TIMESTAMP as default, consistent with 0001 /
--     0003 (NOT the strftime('…','now') literal — the codebase-wide idiom is
--     CURRENT_TIMESTAMP; the repo layer is the source of truth for ISO-8601
--     timestamp format on write).
--   * Severity on `risks` is CHECK-constrained at the DB layer (closed enum:
--     low | medium | high | critical); confidence on `rejected_alternatives`
--     is free TEXT (mirrors research_notes.confidence — validated in repo).
--   * `task_kind` is added via ALTER TABLE ADD COLUMN with a CHECK that
--     admits NULL (per R16 — FK-bearing or otherwise, ALTER-added columns
--     must accept NULL). No default; existing rows remain NULL.

-- ---------------------------------------------------------------------------
-- rejected_alternatives: per-work-item options considered and discarded.
-- Mirrors the 0003 research_notes idiom — TEXT PK, parent FK ON DELETE
-- CASCADE, monotonic `seq` with UNIQUE(work_item_id, seq), self-FK
-- supersession chain (ON DELETE SET NULL so superseding records survive
-- deletion of the superseded row).
--   summary       — short label.
--   body          — optional longer description.
--   rationale     — free-text "why was this rejected".
--   confidence    — high|medium|low (free TEXT, mirrors research_notes; no
--                   CHECK — validated in repo).
--   superseded_by — self-FK to the alternative that supersedes this one; live
--                   alternatives are `superseded_by IS NULL`.
-- ---------------------------------------------------------------------------
CREATE TABLE rejected_alternatives (
    id            TEXT PRIMARY KEY,
    work_item_id  TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    seq           INTEGER NOT NULL,
    summary       TEXT NOT NULL,
    body          TEXT,
    rationale     TEXT,
    confidence    TEXT,
    superseded_by TEXT REFERENCES rejected_alternatives(id) ON DELETE SET NULL,
    created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (work_item_id, seq)
);

CREATE INDEX rejected_alternatives_work_item_seq_idx
    ON rejected_alternatives(work_item_id, seq);

-- ---------------------------------------------------------------------------
-- risks: per-work-item risk register.
-- Same shape as rejected_alternatives, except:
--   severity   — low|medium|high|critical, CHECK-enforced (closed enum at
--                the DB layer; gates sprint composition).
--   mitigation — free-text optional mitigation strategy.
--   (no confidence column — severity replaces it.)
--   superseded_by — self-FK to the risk that supersedes this one; ON DELETE
--                   SET NULL so superseding records survive deletion of the
--                   superseded row.
-- ---------------------------------------------------------------------------
CREATE TABLE risks (
    id            TEXT PRIMARY KEY,
    work_item_id  TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    seq           INTEGER NOT NULL,
    summary       TEXT NOT NULL,
    body          TEXT,
    rationale     TEXT,
    severity      TEXT NOT NULL CHECK (severity IN ('low', 'medium', 'high', 'critical')),
    mitigation    TEXT,
    superseded_by TEXT REFERENCES risks(id) ON DELETE SET NULL,
    created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (work_item_id, seq)
);

CREATE INDEX risks_work_item_seq_idx ON risks(work_item_id, seq);

-- ---------------------------------------------------------------------------
-- task_dependencies: directed edges between tasks.
-- Composite PK (task_id, depends_on_id) makes duplicate edges structurally
-- impossible. Row-level CHECK rejects self-loops (task depending on itself).
-- The BEFORE INSERT trigger below asserts BOTH endpoints reference a
-- work_items row where kind='task' — a column CHECK cannot subquery sibling
-- rows, so a trigger is the authoritative guard (mirrors the 0004 repo_links
-- kind-check trigger pattern).
--   kind — edge category (data | sequence | …); free TEXT, default 'data'.
-- ON DELETE CASCADE on both FKs ties an edge to the lifetime of either
-- endpoint task — deleting either side removes the edge.
-- ---------------------------------------------------------------------------
CREATE TABLE task_dependencies (
    task_id       TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    depends_on_id TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL DEFAULT 'data',
    created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (task_id, depends_on_id),
    CHECK (task_id <> depends_on_id)
);

-- Reverse-lookup index ("who depends on me?") — the forward direction is
-- already covered by the PRIMARY KEY's leading column.
CREATE INDEX task_dependencies_depends_on_idx
    ON task_dependencies(depends_on_id);

-- ---------------------------------------------------------------------------
-- Kind-check trigger: both task_id and depends_on_id must reference a
-- work_items row where kind='task'. Mirrors the 0004 repo_links kind-check
-- trigger shape. A correlated subquery against work_items.kind cannot live
-- in a column CHECK, so a BEFORE INSERT trigger is the authoritative guard.
--
-- Only INSERT is guarded: the PRIMARY KEY columns (task_id, depends_on_id)
-- are not mutated post-insert in the repo layer (edges are insert-or-delete,
-- never updated in place), so a BEFORE UPDATE trigger would be dead code.
-- The `kind` and `created_at` columns are the only updatable fields, and
-- neither carries a hierarchy invariant.
-- ---------------------------------------------------------------------------
CREATE TRIGGER task_dependencies_kind_check
    BEFORE INSERT ON task_dependencies
    FOR EACH ROW WHEN
        (SELECT kind FROM work_items WHERE id = NEW.task_id) <> 'task'
        OR (SELECT kind FROM work_items WHERE id = NEW.depends_on_id) <> 'task'
    BEGIN
        SELECT RAISE(ABORT, 'task_dependencies row requires both sides to reference work_items with kind = ''task''');
    END;

-- ---------------------------------------------------------------------------
-- work_items.task_kind: task-scope discriminator for the sprint composer.
-- Nullable, no default — per R16, ALTER-added columns must accept NULL.
-- The CHECK admits NULL OR one of the four enum literals (foundation |
-- vertical-slice | pattern-replacement | polish). Non-task rows are
-- expected to leave this NULL; the repo layer is the source of truth for
-- "only task rows may carry a task_kind" (no DB-level kind-coupling guard,
-- matching how 0003's task-only `effort`/`complexity` columns are left
-- repo-validated).
-- ---------------------------------------------------------------------------
ALTER TABLE work_items ADD COLUMN task_kind TEXT
    CHECK (task_kind IS NULL OR task_kind IN ('foundation', 'vertical-slice', 'pattern-replacement', 'polish'));
