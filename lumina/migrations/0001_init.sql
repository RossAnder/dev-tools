-- lumina initial schema (Task 2).
--
-- Design constraints:
--   * All `id` columns are TEXT holding an app-generated UUIDv7 (generation
--     lives in the repo layer / tests; the schema only declares TEXT). No
--     integer rowids are relied upon — ids stay stable across DB rebuilds so
--     the `<id>.toml` export filenames and cross-table FKs remain portable.
--   * SQL is kept ANSI-ish for a later Postgres port. The ONE intentional
--     SQLite-specific construct is the hierarchy-enforcement trigger below: a
--     column CHECK cannot subquery the parent row's `kind`, so a BEFORE
--     INSERT / BEFORE UPDATE trigger pair is the authoritative guard (P3).
--   * `status` is free text (no enum / CHECK) in slice 1 (P14) — the importer
--     maps source statuses through verbatim; validation is deferred.

PRAGMA foreign_keys = ON;

-- ---------------------------------------------------------------------------
-- work_items: the 5-level adjacency-list hierarchy
--   project > epic > feature > story > task
-- `parent_id` is a self-FK; the trigger pair below enforces legal
-- (kind, parent-kind) edges.
-- ---------------------------------------------------------------------------
CREATE TABLE work_items (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,
    parent_id  TEXT REFERENCES work_items(id),
    title      TEXT NOT NULL,
    body       TEXT,
    status     TEXT NOT NULL,
    position   INTEGER,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_work_items_parent_id ON work_items(parent_id);
CREATE INDEX idx_work_items_kind ON work_items(kind);

-- ---------------------------------------------------------------------------
-- Hierarchy enforcement (P3) — authoritative guard.
--
-- Legal (kind, parent-kind) pairs:
--   project ← parent_id IS NULL
--   epic    ← parent kind = 'project'
--   feature ← parent kind = 'epic'
--   story   ← parent kind = 'feature'
--   task    ← parent kind = 'story'
-- Any other combination — including a non-`project` with a NULL parent, or a
-- `project` with a non-NULL parent — aborts with a descriptive message.
--
-- The parent's kind is looked up via the correlated subquery
--   (SELECT kind FROM work_items WHERE id = NEW.parent_id)
-- which yields NULL when `parent_id` is NULL or dangling; the NULL-parent
-- branch is handled explicitly so a non-`project` orphan is rejected rather
-- than silently treated as legal.
--
-- Two triggers (INSERT + UPDATE) keep the guard authoritative on every write
-- path; their bodies are byte-identical apart from the trigger event.
-- ---------------------------------------------------------------------------
CREATE TRIGGER trg_work_items_hierarchy_insert
BEFORE INSERT ON work_items
FOR EACH ROW
BEGIN
    SELECT CASE
        WHEN NEW.parent_id IS NULL THEN
            CASE WHEN NEW.kind = 'project' THEN NULL
                 ELSE RAISE(ABORT, 'illegal work_item hierarchy: non-project with NULL parent')
            END
        ELSE
            CASE
                WHEN NEW.kind = 'epic'    AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'project' THEN NULL
                WHEN NEW.kind = 'feature' AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'epic'    THEN NULL
                WHEN NEW.kind = 'story'   AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'feature' THEN NULL
                WHEN NEW.kind = 'task'    AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'story'   THEN NULL
                ELSE RAISE(ABORT, 'illegal work_item hierarchy: child kind not permitted under parent kind')
            END
    END;
END;

CREATE TRIGGER trg_work_items_hierarchy_update
BEFORE UPDATE ON work_items
FOR EACH ROW
BEGIN
    SELECT CASE
        WHEN NEW.parent_id IS NULL THEN
            CASE WHEN NEW.kind = 'project' THEN NULL
                 ELSE RAISE(ABORT, 'illegal work_item hierarchy: non-project with NULL parent')
            END
        ELSE
            CASE
                WHEN NEW.kind = 'epic'    AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'project' THEN NULL
                WHEN NEW.kind = 'feature' AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'epic'    THEN NULL
                WHEN NEW.kind = 'story'   AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'feature' THEN NULL
                WHEN NEW.kind = 'task'    AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'story'   THEN NULL
                ELSE RAISE(ABORT, 'illegal work_item hierarchy: child kind not permitted under parent kind')
            END
    END;
END;

-- ---------------------------------------------------------------------------
-- findings: review / optimise findings attached to a work-item.
-- Disposition fields (resolved_at, resolution, defer_reason, defer_trigger,
-- wontfix_rationale) are included (P7) so deferred / wontfix imports are not
-- lossy.
-- ---------------------------------------------------------------------------
CREATE TABLE findings (
    id                TEXT PRIMARY KEY,
    work_item_id      TEXT REFERENCES work_items(id),
    kind              TEXT,
    severity          TEXT,
    effort            TEXT,
    category          TEXT,
    status            TEXT,
    file              TEXT,
    line              INTEGER,
    symbol            TEXT,
    summary           TEXT,
    description       TEXT,
    first_flagged     TEXT,
    rounds            INTEGER,
    fingerprint       TEXT,
    flow              TEXT,
    dedup_id          TEXT,
    resolved_at       TEXT,
    resolution        TEXT,
    defer_reason      TEXT,
    defer_trigger     TEXT,
    wontfix_rationale TEXT
);

CREATE INDEX idx_findings_work_item_id ON findings(work_item_id);

-- ---------------------------------------------------------------------------
-- events: append-only log doubling as the transactional outbox.
-- `exported_at IS NULL` marks an undrained event; the git-export materialiser
-- (Task 6) drains `WHERE exported_at IS NULL` and stamps `exported_at`.
-- `payload` holds JSON as TEXT.
-- ---------------------------------------------------------------------------
CREATE TABLE events (
    id             TEXT PRIMARY KEY,
    aggregate_type TEXT NOT NULL,
    aggregate_id   TEXT NOT NULL,
    event_type     TEXT NOT NULL,
    payload        TEXT,
    actor          TEXT,
    created_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    exported_at    TEXT
);

-- Partial-ish index over the outbox drain predicate (Task 6 hot path).
CREATE INDEX idx_events_unexported ON events(exported_at);

-- ---------------------------------------------------------------------------
-- context_blocks + work_item_context: the drift-killer.
-- Shared context becomes a single row referenced by many work-items via the
-- link table, so divergence is structurally impossible rather than checked.
-- ---------------------------------------------------------------------------
CREATE TABLE context_blocks (
    id         TEXT PRIMARY KEY,
    title      TEXT,
    body       TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE work_item_context (
    work_item_id     TEXT REFERENCES work_items(id),
    context_block_id TEXT REFERENCES context_blocks(id),
    PRIMARY KEY (work_item_id, context_block_id)
);
