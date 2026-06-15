-- lumina migration 0020: task_files — files_touched promoted to a first-class set.
--
-- ## Why
-- Until now a task's touched files lived only as a JSON array on the task's
-- `attributes` (`files_touched`, optionally per-repo-qualified — migration 0004).
-- That JSON form cannot be indexed, queried, or de-duplicated at the storage
-- layer. This migration promotes it to a child table keyed to the task, with one
-- row per (task × kind × repo × path) and a UNIQUE index enforcing set
-- membership so the same file cannot be recorded twice for the same task/kind.
--
-- ## Two kinds — plan-time vs execution-time
--   * kind='expected' — the PLAN-time set (what the task spec says it will touch).
--   * kind='actual'   — the EXECUTION-time set (what the implementer actually
--                       touched). Keeping both lets a later pass diff plan vs
--                       reality. CHECK-constrained to those two values.
--
-- ## Repo binding mirrors migration 0004's primary-fallback rule
--   * `repo_link_id` is a NULLABLE FK → repo_links(id). NULL means the file lives
--     in the project's PRIMARY linked repo (the same implicit-primary fallback
--     `findings.repo_id` uses in 0004). A non-NULL value qualifies the file to a
--     specific (non-primary) linked repo.
--   * ON DELETE SET NULL (NOT a hard CASCADE): removing a repo link must not
--     delete the file history that pointed at it — those rows fall back to the
--     project's primary, exactly like `findings.repo_id ON DELETE SET NULL`.
--   * `task_id` by contrast IS ON DELETE CASCADE: a task_files row is meaningless
--     without its task, so it dies with the task.
--
-- ## Set-membership UNIQUE — COALESCE expression index
-- A plain `UNIQUE(task_id, kind, repo_link_id, path)` would NOT dedupe rows whose
-- `repo_link_id` is NULL, because SQLite treats NULLs as pairwise-distinct in a
-- UNIQUE constraint — two identical primary-repo files (both NULL repo_link_id)
-- would both insert. The EXPRESSION index over `COALESCE(repo_link_id, '')`
-- collapses NULL to '' so the primary-repo bucket dedupes too. A violation
-- reports `UNIQUE constraint failed: index 'idx_task_files_unique'` (names the
-- INDEX, not a column path — the expression-index shape, as in 0019).
--
-- ## Design constraints (mirroring 0001-0019)
--   * `id` is TEXT holding an app-generated UUIDv7 (generation in the repo layer).
--   * `created_at` defaults CURRENT_TIMESTAMP, consistent with 0001/0003/0005.
--   * Creating a FRESH table, so full FK clauses with ON DELETE actions are legal
--     (the SQLite `ALTER TABLE ADD COLUMN … REFERENCES` NULL-default restriction
--     applies only to column-add, not to CREATE TABLE).
-- Forward-only; no down-migration.

PRAGMA foreign_keys = ON;

-- ---------------------------------------------------------------------------
-- task_files: one row per (task × kind × repo × path).
--   task_id      — FK to work_items(id); ON DELETE CASCADE (dies with the task).
--   repo_link_id — nullable FK to repo_links(id); ON DELETE SET NULL.
--                  NULL ⇒ the file lives in the project's PRIMARY linked repo.
--   path         — the repo-relative file path.
--   kind         — 'expected' (plan-time set) or 'actual' (execution-time set).
-- ---------------------------------------------------------------------------
CREATE TABLE task_files (
    id           TEXT NOT NULL PRIMARY KEY,
    task_id      TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    repo_link_id TEXT          REFERENCES repo_links(id) ON DELETE SET NULL,
    path         TEXT NOT NULL,
    kind         TEXT NOT NULL CHECK (kind IN ('expected', 'actual')),
    created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Set membership: the same (task, kind, repo, path) cannot be recorded twice.
-- COALESCE collapses NULL repo_link_id to '' so primary-repo rows dedupe too
-- (SQLite treats plain UNIQUE NULLs as distinct — hence the expression index).
CREATE UNIQUE INDEX idx_task_files_unique
    ON task_files(task_id, kind, COALESCE(repo_link_id, ''), path);

-- Per-task lookup hot path (list a task's files, optionally narrowed by kind).
CREATE INDEX idx_task_files_task ON task_files(task_id, kind);
