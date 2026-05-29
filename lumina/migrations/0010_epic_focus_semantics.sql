-- lumina migration 0010: epic/focus semantics (schema-only).
--
-- Forward-only; no down-migration. Recovery if a later task fails: `git revert`
-- this file and `cargo sqlx database reset` on a fresh DB (decision 1: no live
-- data, so a wipe-and-recreate is acceptable).
--
-- ## Why
-- `epic` and `feature` were byte-identical hierarchy levels distinguished only
-- by depth. The `feature` level is renamed to `focus` (the Rust rename
-- Kind::Feature -> Kind::Focus lands in the same wave) and `focus` items gain a
-- `shape` axis. This migration carries the schema half of that change.
--
-- ## Triggers
-- The LIVE hierarchy-guard triggers were last recreated by migration 0007 (its
-- work_items table-rebuild superseded the 0001 originals). We DROP both BY NAME
-- (they exist regardless of which migration last created them) and recreate them
-- from the 0007 bodies, changing ONLY the kind literals so the epic->child and
-- child->story edges reference 'focus' instead of 'feature'. The two blocks are
-- inlined verbatim so the byte-identical pair cannot drift. The attributes-
-- validity triggers and the repo_links / task_dependencies kind-check triggers
-- contain no 'feature' literal and are left untouched.
--
-- ## shape column
-- ADD COLUMN with a CHECK is legal since SQLite 3.37.0 (2021-11-27). The column
-- is nullable because ADD COLUMN cannot be NOT NULL without a default; the
-- shape-mandatory-for-focus rule is enforced in the repo create/update path, not
-- by this column.

DROP TRIGGER trg_work_items_hierarchy_insert;
DROP TRIGGER trg_work_items_hierarchy_update;

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
                WHEN NEW.kind = 'epic'  AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'project' THEN NULL
                WHEN NEW.kind = 'focus' AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'epic'    THEN NULL
                WHEN NEW.kind = 'story' AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'focus'   THEN NULL
                WHEN NEW.kind = 'task'  AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'story'   THEN NULL
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
                WHEN NEW.kind = 'epic'  AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'project' THEN NULL
                WHEN NEW.kind = 'focus' AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'epic'    THEN NULL
                WHEN NEW.kind = 'story' AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'focus'   THEN NULL
                WHEN NEW.kind = 'task'  AND (SELECT kind FROM work_items WHERE id = NEW.parent_id) = 'story'   THEN NULL
                ELSE RAISE(ABORT, 'illegal work_item hierarchy: child kind not permitted under parent kind')
            END
    END;
END;

ALTER TABLE work_items ADD COLUMN shape TEXT
  CHECK (shape IS NULL OR shape IN ('vertical-slice', 'cross-cutting', 'foundational'));
