-- Requires SQLite >=3.38 (rebuild via the standard 12-step CREATE/INSERT/
-- DROP/RENAME idiom; no production schema features beyond migration 0001's
-- baseline are needed). Forward-only; no down-migration.
--
-- lumina migration 0007: TaskKind variant cull (round-3.5 review follow-up).
--
-- ## Why
-- The migration-0005 task_kind taxonomy mixed task-level dispositions
-- (`foundation`, `polish` — what role does this single task play in its
-- phase?) with intra-story task-subset groupings (`vertical-slice`,
-- `pattern-replacement` — labels for arbitrary subsets of a story's tasks
-- that ship as one unit-of-implementation: implement + test + commit
-- together). The two belong at different granularities — a single task is
-- foundation OR main OR polish; a *subset* of a story's tasks may form a
-- vertical slice or a pattern-replacement bundle (a story may contain
-- multiple such groupings, and a task may belong to zero or more). The
-- review (R7 follow-up discussion) confirmed the conflation:
-- `task_kind_sort_key` (repo.rs) treats the four values as a simple sort
-- hint, and the only mechanical consumer is intra-phase tie-break ordering.
-- The grouping variants were dressing — they leaked subset semantics onto
-- the per-task column.
--
-- This migration narrows TaskKind to the genuinely task-level set —
-- `foundation | main | polish` — and rebuilds the `work_items` table so
-- the column-level CHECK admits the new vocabulary. Existing rows carrying
-- the two deprecated values are migrated to `main` (the closest equivalent:
-- they were originally meant to describe a task's main body of work).
--
-- ## The rebuild
-- SQLite cannot ALTER a CHECK constraint in place (no `ALTER TABLE ALTER
-- COLUMN`). The standard CREATE/INSERT/DROP/RENAME dance is the only path.
-- `PRAGMA defer_foreign_keys = ON` is set at the migration start so the
-- DROP+RENAME window does not trip the inbound FK references from
-- `work_item_activity`, `findings`, `acceptance_criteria`, `research_notes`,
-- `open_questions`, `question_options`, `repo_links`, `risks`,
-- `rejected_alternatives`, and `task_dependencies` — the references re-bind
-- by table name after the RENAME, and the deferred check at COMMIT confirms
-- nothing is dangling.
--
-- Triggers (`trg_work_items_hierarchy_{insert,update}` from migration 0001
-- and `trg_work_items_attributes_{insert,update}` from migration 0002) are
-- dropped with the old table and recreated verbatim against the new one.
-- The two indices (`idx_work_items_parent_id`, `idx_work_items_kind`) are
-- recreated the same way.
--
-- ## Intra-story task-subset groupings are intentionally NOT modelled
-- "Vertical slice" and "pattern replacement" remain real concepts but
-- live at the *task-subset* granularity (see CONVENTIONS §j.1) — one
-- story may contain multiple vertical-slice groupings each spanning a
-- different subset of its tasks, plus multiple pattern-replacement
-- bundles, plus tasks that belong to no grouping. If/when a concrete
-- consumer needs to query groupings from the DB (e.g. `/lumina:run-batch`
-- choosing "dispatch these three together as one verification + one
-- commit"), a future migration adds `task_groups (id, story_id, kind,
-- label)` + `task_group_members (group_id, task_id, seq)`. Until then
-- groupings live purely in `/lumina:decompose-tasks` proposal prose; the
-- only DB-level trace of pattern-replacement membership is
-- `attributes.files_touched_pattern` on each member task. Round 3.5
-- ships the cull only.

PRAGMA defer_foreign_keys = ON;

-- 0. Drop the migration-0004 triggers on `repo_links` that reference
--    `work_items.kind` in their bodies — SQLite validates trigger bodies
--    when the referenced table is mutated, so leaving them in place across
--    the DROP TABLE work_items below trips a "no such table: work_items"
--    error before the RENAME re-binds the name. We recreate them verbatim
--    after the rebuild completes.
DROP TRIGGER repo_links_kind_check_insert;
DROP TRIGGER repo_links_kind_check_update;

-- Same hazard applies to the migration-0005 trigger on `task_dependencies`
-- which also references `work_items.kind` for the kind-check guard.
DROP TRIGGER task_dependencies_kind_check;

-- 1. Create the new table with the narrowed CHECK. Column order, types,
--    NOT-NULL constraints, defaults, and the self-FK on `parent_id` mirror
--    the cumulative effect of migrations 0001 + 0002 + 0003 + 0005 + 0006
--    on the work_items table. `task_kind`'s CHECK is the only narrowed bit.
CREATE TABLE work_items_new (
    id                       TEXT PRIMARY KEY,
    kind                     TEXT NOT NULL,
    parent_id                TEXT REFERENCES work_items(id),
    title                    TEXT NOT NULL,
    body                     TEXT,
    status                   TEXT NOT NULL,
    position                 INTEGER,
    created_at               TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at               TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    attributes               TEXT,
    deleted_at               TEXT,
    relevance                TEXT,
    effort                   TEXT,
    complexity               TEXT,
    origin                   TEXT,
    closure_gate             TEXT,
    blocked_by_question_id   TEXT,
    enabling_option_id       TEXT,
    task_kind                TEXT
        CHECK (task_kind IS NULL OR task_kind IN ('foundation', 'main', 'polish')),
    tier                     TEXT
        CHECK (tier IS NULL OR tier IN ('lite', 'deep'))
);

-- 2. Copy rows, mapping deprecated task_kind values to 'main'. The
--    deprecated values were per-task labels that overloaded subset-grouping
--    semantics onto the task-level column; the actual task-level role of
--    those rows (the role this column should describe) is overwhelmingly
--    "main body of work, not prerequisite, not after-work" — so `main` is
--    the correct task-level disposition for every row that had previously
--    been mislabelled with a grouping shorthand. Any pattern-replacement
--    grouping membership the original row implied lives instead on
--    `attributes.files_touched_pattern` (set by `/lumina:decompose-tasks`).
INSERT INTO work_items_new (
    id, kind, parent_id, title, body, status, position,
    created_at, updated_at, attributes, deleted_at,
    relevance, effort, complexity, origin, closure_gate,
    blocked_by_question_id, enabling_option_id,
    task_kind, tier
)
SELECT
    id, kind, parent_id, title, body, status, position,
    created_at, updated_at, attributes, deleted_at,
    relevance, effort, complexity, origin, closure_gate,
    blocked_by_question_id, enabling_option_id,
    CASE task_kind
        WHEN 'vertical-slice'      THEN 'main'
        WHEN 'pattern-replacement' THEN 'main'
        ELSE task_kind
    END,
    tier
FROM work_items;

-- 3. Drop the old table. The four triggers attached to it are dropped
--    automatically (per SQLite semantics for table drops). FK references
--    from other tables remain valid syntactically; defer_foreign_keys
--    ensures no integrity check fires until COMMIT.
DROP TABLE work_items;

-- 4. Rename the new table into place. After this, the inbound FKs from
--    other tables re-bind by name; the next COMMIT validates them.
ALTER TABLE work_items_new RENAME TO work_items;

-- 5. Recreate the two indices from migration 0001.
CREATE INDEX idx_work_items_parent_id ON work_items(parent_id);
CREATE INDEX idx_work_items_kind ON work_items(kind);

-- 6. Recreate the hierarchy-guard triggers from migration 0001 verbatim.
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

-- 7. Recreate the attributes-validity triggers from migration 0002 verbatim.
CREATE TRIGGER trg_work_items_attributes_insert
BEFORE INSERT ON work_items
FOR EACH ROW
WHEN NEW.attributes IS NOT NULL AND NOT json_valid(NEW.attributes)
BEGIN
    SELECT RAISE(ABORT, 'invalid work_item attributes: not valid JSON');
END;

CREATE TRIGGER trg_work_items_attributes_update
BEFORE UPDATE ON work_items
FOR EACH ROW
WHEN NEW.attributes IS NOT NULL AND NOT json_valid(NEW.attributes)
BEGIN
    SELECT RAISE(ABORT, 'invalid work_item attributes: not valid JSON');
END;

-- 8. Recreate the migration-0004 triggers on `repo_links` verbatim.
CREATE TRIGGER repo_links_kind_check_insert
    BEFORE INSERT ON repo_links
    FOR EACH ROW WHEN (SELECT kind FROM work_items WHERE id = NEW.project_id) <> 'project'
    BEGIN SELECT RAISE(ABORT, 'repo_links.project_id must reference a work_item where kind=project'); END;

CREATE TRIGGER repo_links_kind_check_update
    BEFORE UPDATE ON repo_links
    FOR EACH ROW WHEN (SELECT kind FROM work_items WHERE id = NEW.project_id) <> 'project'
    BEGIN SELECT RAISE(ABORT, 'repo_links.project_id must reference a work_item where kind=project'); END;

-- 9. Recreate the migration-0005 trigger on `task_dependencies` verbatim.
CREATE TRIGGER task_dependencies_kind_check
    BEFORE INSERT ON task_dependencies
    FOR EACH ROW WHEN
        (SELECT kind FROM work_items WHERE id = NEW.task_id) <> 'task'
        OR (SELECT kind FROM work_items WHERE id = NEW.depends_on_id) <> 'task'
    BEGIN
        SELECT RAISE(ABORT, 'task_dependencies row requires both sides to reference work_items with kind = ''task''');
    END;
