-- lumina migration 0004: project↔repo links (T1 of lumina-project-repo-links).
--
-- Additive / non-destructive over 0001-0003. Two classes of change:
--   1. New `repo_links` child table keyed to `work_items(kind='project')` — one
--      row per (project × linked GitHub repo). Slugs are stored canonically
--      lowercased; a single "primary" per project is enforced by a partial
--      UNIQUE index on `(project_id) WHERE is_primary = 1`. The kind-check
--      trigger pair (BEFORE INSERT + BEFORE UPDATE) mirrors the hierarchy
--      trigger pair in 0001_init.sql:59-97 and is the authoritative guard that
--      a `repo_links` row may only attach to a `project` work-item.
--   2. New nullable `findings.repo_id` FK pointing at `repo_links(id)` —
--      qualifies a finding's `file:line` to a non-primary linked repo; NULL =
--      resolves to the project's primary linked repo.
--
-- Design constraints (mirroring 0001-0003):
--   * SQL kept ANSI-ish for a later Postgres port; the trigger pair is the one
--     intentional SQLite-specific construct (correlated subquery against the
--     parent's `kind` cannot live in a column CHECK).
--   * All `id` columns are TEXT holding an app-generated UUIDv7.
--   * SQLite ALTER TABLE ADD COLUMN with REFERENCES is legal ONLY with its
--     implicit NULL default — adding a non-NULL DEFAULT to such a column would
--     abort under foreign_keys=ON. `findings.repo_id` therefore carries NO
--     DEFAULT.

-- ---------------------------------------------------------------------------
-- repo_links: one row per (project × linked GitHub repo).
--   project_id  — FK to work_items(id); kind-check trigger asserts kind='project'.
--   slug        — '<owner>/<name>', both segments lowercased by the repo layer
--                 before INSERT. UNIQUE(project_id, slug) makes duplicate links
--                 structurally impossible.
--   position    — sibling-ordering integer (repo layer allocates MAX+1).
--   is_primary  — 0/1 flag; the partial UNIQUE index below enforces at most one
--                 primary per project.
-- ON DELETE CASCADE ties a project's links to its lifetime.
-- ---------------------------------------------------------------------------
CREATE TABLE repo_links (
    id         TEXT    PRIMARY KEY,
    project_id TEXT    NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    slug       TEXT    NOT NULL,
    position   INTEGER NOT NULL,
    is_primary INTEGER NOT NULL DEFAULT 0 CHECK (is_primary IN (0, 1)),
    created_at TEXT    NOT NULL,
    UNIQUE (project_id, slug)
);

-- At most one primary per project (partial UNIQUE index — SQLite supports this).
CREATE UNIQUE INDEX idx_repo_links_one_primary
    ON repo_links(project_id) WHERE is_primary = 1;

-- Per-project ordering hot path.
CREATE INDEX idx_repo_links_project ON repo_links(project_id, position);

-- ---------------------------------------------------------------------------
-- Kind-check trigger pair: project_id must reference a work_item where
-- kind='project'. Mirrors the BEFORE INSERT + BEFORE UPDATE shape of the
-- hierarchy trigger pair in 0001_init.sql:59-97.
-- ---------------------------------------------------------------------------
CREATE TRIGGER repo_links_kind_check_insert
    BEFORE INSERT ON repo_links
    FOR EACH ROW WHEN (SELECT kind FROM work_items WHERE id = NEW.project_id) <> 'project'
    BEGIN SELECT RAISE(ABORT, 'repo_links.project_id must reference a work_item where kind=project'); END;

CREATE TRIGGER repo_links_kind_check_update
    BEFORE UPDATE ON repo_links
    FOR EACH ROW WHEN (SELECT kind FROM work_items WHERE id = NEW.project_id) <> 'project'
    BEGIN SELECT RAISE(ABORT, 'repo_links.project_id must reference a work_item where kind=project'); END;

-- ---------------------------------------------------------------------------
-- findings.repo_id: nullable FK to repo_links(id). NULL ⇒ the finding's
-- file:line resolves to the project's primary linked repo. ON DELETE SET NULL
-- so removing a repo link does not cascade-delete the findings that pointed at
-- it — they fall back to the project's primary instead.
--
-- NO DEFAULT here: ADD COLUMN ... REFERENCES is legal only with the implicit
-- NULL default under foreign_keys=ON.
-- ---------------------------------------------------------------------------
ALTER TABLE findings ADD COLUMN repo_id TEXT REFERENCES repo_links(id) ON DELETE SET NULL;
