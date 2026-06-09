//! Migration 0016 acceptance test (sprint-lifecycle & worktree substrate, schema-only).
//!
//! Proves the purely-additive migration applies cleanly to a fresh,
//! fully-migrated in-memory DB and that the new schema objects exist and behave
//! as designed:
//!   (a) The migration applies cleanly — `connect_in_memory` (which runs every
//!       embedded migration including 0016) succeeds.
//!   (b) Both new tables exist (`worktrees`, `task_commits`), the two new
//!       `sprints` columns (`worktree_id`, `predecessor_sprint_id`), the new
//!       `work_items.checkpoint` column, and the expected `worktrees` /
//!       `task_commits` columns are present.
//!   (c) The `worktrees.outcome` CHECK rejects an out-of-vocab value while
//!       `'merged'` / `'rejected'` / NULL succeed.
//!   (d) The `worktrees.owning_sprint_id` UNIQUE rejects a second worktree owned
//!       by the same sprint.
//!   (e) No `sprints` row holds an out-of-vocab status after the migration —
//!       `SELECT COUNT(*) FROM sprints WHERE status NOT IN (vocab)` == 0
//!       (mechanically gates the `'open'`→`'active'` backfill). The migration has
//!       already run on the empty `sprints` of `connect_in_memory`, so the test
//!       seeds a fresh legacy `'open'` row, re-runs the same forward-only backfill
//!       UPDATE the migration embeds, asserts it is now `'active'`, and asserts
//!       zero out-of-vocab rows table-wide.
//!   (f) The partial index `idx_sprints_worktree` and the `ux_task_commits`
//!       UNIQUE index are actually USED (SEARCH not SCAN) by a representative
//!       query against each.
//!
//! The pool comes from `db::connect_in_memory`, which enables
//! `foreign_keys(true)` per-connection. All assertions use the RUNTIME
//! `sqlx::query` / `query_scalar` string API (NOT compile-checked macros), so
//! this test introduces no `.sqlx/` cache entry — matches `migration_0013.rs`.

use lumina::db::connect_in_memory;
use sqlx::Row as _;
use sqlx::SqlitePool;

/// COUNT of `sqlite_master` rows of a given `type` (table/index) with `name`.
/// Used to assert object existence (== 1).
async fn master_count(pool: &SqlitePool, kind: &str, name: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = ? AND name = ?")
        .bind(kind)
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("inspect sqlite_master")
}

/// COUNT of `pragma_table_info` rows for a column on a table — 1 when present.
/// The table name is passed as a BOUND argument to the `pragma_table_info(?)`
/// table-valued function so the SQL stays a `&'static str` literal (sqlx 0.9's
/// `SqlSafeStr` bound rejects a dynamically-built `&String`).
async fn shape_col(pool: &SqlitePool, table: &str, col: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM pragma_table_info(?) WHERE name = ?")
        .bind(table)
        .bind(col)
        .fetch_one(pool)
        .await
        .expect("inspect table columns")
}

/// Raw INSERT of a `project` work_item (the hierarchy trigger requires a
/// `project` to carry a NULL parent). A single project row satisfies the FK
/// target used by `task_commits.task_id` here.
async fn insert_project(
    pool: &SqlitePool,
    id: &str,
) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
    sqlx::query("INSERT INTO work_items (id, kind, parent_id, title, status) VALUES (?, ?, ?, ?, ?)")
        .bind(id)
        .bind("project")
        .bind(None::<&str>)
        .bind("project title")
        .bind("open")
        .execute(pool)
        .await
}

/// Raw INSERT of a `sprints` row with an explicit `status` (migration-0011
/// table; `sprints.status` is free TEXT). Returns the result so callers can
/// assert success / failure.
async fn insert_sprint(
    pool: &SqlitePool,
    id: &str,
    status: &str,
) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
    sqlx::query("INSERT INTO sprints (id, title, status) VALUES (?, ?, ?)")
        .bind(id)
        .bind("a sprint")
        .bind(status)
        .execute(pool)
        .await
}

/// Raw INSERT of a `worktrees` row with an explicit `outcome` (NULL or a value).
/// Built so the `outcome` CHECK / `owning_sprint_id` UNIQUE are the things under
/// test; `path` is the only other NOT-NULL column.
async fn insert_worktree(
    pool: &SqlitePool,
    id: &str,
    owning_sprint_id: &str,
    outcome: Option<&str>,
) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO worktrees (id, owning_sprint_id, path, outcome) VALUES (?, ?, ?, ?)",
    )
    .bind(id)
    .bind(owning_sprint_id)
    .bind("/tmp/wt")
    .bind(outcome)
    .execute(pool)
    .await
}

#[tokio::test]
async fn migration_0016_sprint_lifecycle_worktrees() {
    // (a) The whole embedded migration set (through 0016) applies on a fresh
    //     in-memory DB.
    let pool = connect_in_memory()
        .await
        .expect("migrated in-memory pool (0016 applied cleanly)");

    // (b) Both new tables exist.
    for table in ["worktrees", "task_commits"] {
        assert_eq!(
            master_count(&pool, "table", table).await,
            1,
            "migration-0016 table `{table}` is present"
        );
    }

    // The two new `sprints` columns + the new `work_items.checkpoint` column.
    for col in ["worktree_id", "predecessor_sprint_id"] {
        assert_eq!(
            shape_col(&pool, "sprints", col).await,
            1,
            "migration-0016 `sprints.{col}` column is present"
        );
    }
    assert_eq!(
        shape_col(&pool, "work_items", "checkpoint").await,
        1,
        "migration-0016 `work_items.checkpoint` column is present"
    );

    // The expected `worktrees` columns are present.
    for col in [
        "id",
        "owning_sprint_id",
        "path",
        "base_ref",
        "branch",
        "merged_at",
        "merge_ref",
        "outcome",
        "created_at",
        "updated_at",
        "deleted_at",
    ] {
        assert_eq!(
            shape_col(&pool, "worktrees", col).await,
            1,
            "migration-0016 `worktrees.{col}` column is present"
        );
    }

    // The expected `task_commits` columns are present.
    for col in ["id", "commit_sha", "task_id", "sprint_id", "recorded_at"] {
        assert_eq!(
            shape_col(&pool, "task_commits", col).await,
            1,
            "migration-0016 `task_commits.{col}` column is present"
        );
    }

    // All four new indexes exist.
    for index in [
        "idx_sprints_worktree",
        "idx_task_commits_task",
        "idx_task_commits_commit",
        "ux_task_commits",
    ] {
        assert_eq!(
            master_count(&pool, "index", index).await,
            1,
            "migration-0016 index `{index}` is present"
        );
    }

    // (c) The `worktrees.outcome` CHECK is enforced. Seed an owning sprint per
    //     worktree (owning_sprint_id is a NOT-NULL UNIQUE FK → sprints).
    insert_sprint(&pool, "sp-merged", "review")
        .await
        .expect("seed owning sprint sp-merged");
    insert_worktree(&pool, "wt-merged", "sp-merged", Some("merged"))
        .await
        .expect("outcome='merged' is accepted");

    insert_sprint(&pool, "sp-rejected", "review")
        .await
        .expect("seed owning sprint sp-rejected");
    insert_worktree(&pool, "wt-rejected", "sp-rejected", Some("rejected"))
        .await
        .expect("outcome='rejected' is accepted");

    insert_sprint(&pool, "sp-null", "active")
        .await
        .expect("seed owning sprint sp-null");
    insert_worktree(&pool, "wt-null", "sp-null", None)
        .await
        .expect("a NULL outcome is accepted (worktree not yet merged/rejected)");

    insert_sprint(&pool, "sp-bogus", "active")
        .await
        .expect("seed owning sprint sp-bogus");
    let bad_outcome = insert_worktree(&pool, "wt-bogus", "sp-bogus", Some("bogus")).await;
    assert!(
        bad_outcome.is_err(),
        "the worktrees.outcome CHECK rejects an out-of-vocabulary value on INSERT"
    );
    // And on UPDATE too (mutating a live worktree to a bogus outcome).
    let bad_update = sqlx::query("UPDATE worktrees SET outcome = ? WHERE id = ?")
        .bind("bogus")
        .bind("wt-null")
        .execute(&pool)
        .await;
    assert!(
        bad_update.is_err(),
        "the worktrees.outcome CHECK rejects an out-of-vocabulary value on UPDATE"
    );

    // (d) The `worktrees.owning_sprint_id` UNIQUE rejects a second worktree
    //     owned by the SAME sprint (1:1 owner invariant). `sp-null` already owns
    //     `wt-null`; a second worktree on `sp-null` must fail.
    let dup_owner = insert_worktree(&pool, "wt-dup", "sp-null", None).await;
    assert!(
        dup_owner.is_err(),
        "the owning_sprint_id UNIQUE rejects a second worktree owned by the same sprint"
    );

    // (e) No `sprints` row holds an out-of-vocab status — and the `'open'`→
    //     `'active'` backfill works. `connect_in_memory` already ran the
    //     migration on an empty `sprints`, so to exercise the backfill we seed a
    //     fresh legacy `'open'` sprint, re-run the SAME forward-only UPDATE the
    //     migration embeds, and assert it is now `'active'` and that zero rows
    //     hold an out-of-vocab status across the whole table.
    insert_sprint(&pool, "sp-legacy-open", "open")
        .await
        .expect("seed a legacy 'open' sprint");
    sqlx::query("UPDATE sprints SET status = 'active' WHERE status = 'open'")
        .execute(&pool)
        .await
        .expect("re-run the embedded 'open'->'active' backfill");
    let legacy_status: String =
        sqlx::query_scalar("SELECT status FROM sprints WHERE id = ?")
            .bind("sp-legacy-open")
            .fetch_one(&pool)
            .await
            .expect("read back the backfilled legacy sprint");
    assert_eq!(
        legacy_status, "active",
        "the 'open'->'active' backfill renames a legacy 'open' sprint to 'active'"
    );

    let out_of_vocab: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sprints \
         WHERE status NOT IN ('draft', 'ready', 'active', 'review', 'done', 'cancelled')",
    )
    .fetch_one(&pool)
    .await
    .expect("count out-of-vocab sprint statuses");
    assert_eq!(
        out_of_vocab, 0,
        "no sprints row holds an out-of-vocabulary status after the backfill"
    );

    // (f) The partial index `idx_sprints_worktree` is USED by a representative
    //     by-worktree lookup. The query filters `worktree_id = ?` (the partial
    //     predicate is `worktree_id IS NOT NULL`, which a `= ?` equality
    //     satisfies), so the planner can SEARCH sprints via the partial index
    //     instead of SCANning the table. Static literal (sqlx 0.9's `SqlSafeStr`
    //     bound rejects a dynamically-built `&String`, so the
    //     `EXPLAIN QUERY PLAN` prefix is baked into the literal — the crate-wide
    //     idiom: static SQL + bound args).
    const SPRINTS_EXPLAIN_SQL: &str = "EXPLAIN QUERY PLAN \
         SELECT id FROM sprints WHERE worktree_id = ?";
    let sprints_plan = sqlx::query(SPRINTS_EXPLAIN_SQL)
        .bind("wt-merged")
        .fetch_all(&pool)
        .await
        .expect("EXPLAIN QUERY PLAN on the by-worktree sprint lookup");
    let sprints_plan_text: String = sprints_plan
        .iter()
        .map(|r| r.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        sprints_plan_text.contains("idx_sprints_worktree"),
        "expected the partial index `idx_sprints_worktree` in the query plan, got:\n{sprints_plan_text}"
    );
    assert!(
        !sprints_plan_text.contains("SCAN sprints"),
        "expected sprints to be SEARCHed via the partial index, not full-SCANned; plan:\n{sprints_plan_text}"
    );

    // The `ux_task_commits` UNIQUE index is USED by a representative
    // (commit_sha, task_id) lookup. Seed a project (a legal task_id FK target)
    // and a task_commits row so the query has a real shape.
    insert_project(&pool, "p-commit")
        .await
        .expect("legal project under NULL parent");
    sqlx::query(
        "INSERT INTO task_commits (id, commit_sha, task_id, sprint_id) VALUES (?, ?, ?, ?)",
    )
    .bind("tc-1")
    .bind("deadbeef")
    .bind("p-commit")
    .bind(None::<&str>)
    .execute(&pool)
    .await
    .expect("seed a task_commits row");

    const COMMITS_EXPLAIN_SQL: &str = "EXPLAIN QUERY PLAN \
         SELECT id FROM task_commits WHERE commit_sha = ? AND task_id = ?";
    let commits_plan = sqlx::query(COMMITS_EXPLAIN_SQL)
        .bind("deadbeef")
        .bind("p-commit")
        .fetch_all(&pool)
        .await
        .expect("EXPLAIN QUERY PLAN on the (commit_sha, task_id) lookup");
    let commits_plan_text: String = commits_plan
        .iter()
        .map(|r| r.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        commits_plan_text.contains("ux_task_commits"),
        "expected the UNIQUE index `ux_task_commits` in the query plan, got:\n{commits_plan_text}"
    );
    assert!(
        !commits_plan_text.contains("SCAN task_commits"),
        "expected task_commits to be SEARCHed via the unique index, not full-SCANned; plan:\n{commits_plan_text}"
    );
}
