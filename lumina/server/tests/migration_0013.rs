//! Migration 0013 acceptance test (team-execution queue, schema-only).
//!
//! Proves the purely-additive migration applies cleanly to a fresh,
//! fully-migrated in-memory DB and that the new schema objects exist and behave
//! as designed:
//!   (a) The migration applies cleanly — `connect_in_memory` (which runs every
//!       embedded migration including 0013) succeeds.
//!   (b) The four new `work_items` columns are present: `assignee`,
//!       `lease_expires_at`, `lane`, `reviews_work_item_id`.
//!   (c) Both new indexes exist: `idx_work_items_claim` (claim hot path) and
//!       `idx_work_items_lease` (lazy reclaim).
//!   (d) The `lane` CHECK is enforced: `lane='bogus'` is rejected on both INSERT
//!       and UPDATE, while `lane IN ('implement','review')` and `lane NULL`
//!       succeed (back-compat: pre-existing rows leave `lane` NULL).
//!   (e) The claim hot-path partial index `idx_work_items_claim` is actually
//!       USED by a representative claim-candidate SELECT (work_items joined to
//!       sprint_tasks, filtering `lane = ? AND status = 'todo' AND
//!       deleted_at IS NULL`): the query plan SEARCHes work_items via the index
//!       rather than SCANning the table.
//!
//! The pool comes from `db::connect_in_memory`, which enables
//! `foreign_keys(true)` per-connection. All assertions use the RUNTIME
//! `sqlx::query` / `query_scalar` string API (NOT compile-checked macros), so
//! this test introduces no `.sqlx/` cache entry — matches `migration_0011.rs`.

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
/// `project` to carry a NULL parent). A single project row satisfies the FK /
/// chain targets used here.
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

/// Raw INSERT of a work_item with an explicit `lane`. Built so the `lane` CHECK
/// is the only thing under test; the row is a legal `project` (NULL parent).
async fn insert_with_lane(
    pool: &SqlitePool,
    id: &str,
    lane: Option<&str>,
) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO work_items (id, kind, parent_id, title, status, lane) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind("project")
    .bind(None::<&str>)
    .bind("project title")
    .bind("open")
    .bind(lane)
    .execute(pool)
    .await
}

#[tokio::test]
async fn migration_0013_team_execution() {
    // (a) The whole embedded migration set (through 0013) applies on a fresh
    //     in-memory DB.
    let pool = connect_in_memory()
        .await
        .expect("migrated in-memory pool (0013 applied cleanly)");

    // (b) The four new work_items columns are present.
    for col in ["assignee", "lease_expires_at", "lane", "reviews_work_item_id"] {
        assert_eq!(
            shape_col(&pool, "work_items", col).await,
            1,
            "migration-0013 `work_items.{col}` column is present"
        );
    }

    // (c) Both new indexes exist.
    for index in ["idx_work_items_claim", "idx_work_items_lease"] {
        assert_eq!(
            master_count(&pool, "index", index).await,
            1,
            "migration-0013 index `{index}` is present"
        );
    }

    // (d) The `lane` CHECK is enforced.
    //
    // NULL lane succeeds (back-compat: this is what every pre-existing row
    // takes).
    insert_with_lane(&pool, "wi-lane-null", None)
        .await
        .expect("a NULL lane is accepted (not team-managed)");
    // Both in-vocab lane values succeed.
    insert_with_lane(&pool, "wi-lane-impl", Some("implement"))
        .await
        .expect("lane='implement' is accepted");
    insert_with_lane(&pool, "wi-lane-review", Some("review"))
        .await
        .expect("lane='review' is accepted");
    // An out-of-vocab lane is rejected on INSERT.
    let bad_insert = insert_with_lane(&pool, "wi-lane-bogus", Some("bogus")).await;
    assert!(
        bad_insert.is_err(),
        "the lane CHECK rejects an out-of-vocabulary value on INSERT"
    );
    // An out-of-vocab lane is rejected on UPDATE too (mutating a live row).
    let bad_update = sqlx::query("UPDATE work_items SET lane = ? WHERE id = ?")
        .bind("bogus")
        .bind("wi-lane-impl")
        .execute(&pool)
        .await;
    assert!(
        bad_update.is_err(),
        "the lane CHECK rejects an out-of-vocabulary value on UPDATE"
    );

    // (e) The claim hot-path partial index is USED by a representative
    //     claim-candidate SELECT.
    //
    // Seed a sprint + a task in it so the join in the candidate query has a
    // real shape (project → epic → focus → story → task chain, then a sprint
    // and a sprint_tasks membership row).
    insert_project(&pool, "p-claim")
        .await
        .expect("legal project under NULL parent");
    seed_task_in_sprint(&pool).await;

    // The representative claim-candidate query: work_items (the driving table we
    // want SEARCHed via idx_work_items_claim) joined to sprint_tasks, filtering
    // on the SAME predicate columns the future T4 claim uses —
    // `lane = ? AND status = 'todo' AND deleted_at IS NULL` (plus a sprint
    // scope via the join). `deleted_at IS NULL` is the partial-index predicate,
    // and `lane = ?` is the index's leftmost equality term, so the planner can
    // serve work_items from the partial index.
    // Static literal (sqlx 0.9's `SqlSafeStr` bound rejects a dynamically-built
    // `&String`, so the `EXPLAIN QUERY PLAN` prefix is baked into the literal
    // rather than `format!`-ed on — the crate-wide idiom: static SQL + bound args).
    const CANDIDATE_EXPLAIN_SQL: &str = "EXPLAIN QUERY PLAN \
         SELECT wi.id \
         FROM work_items AS wi \
         JOIN sprint_tasks AS st ON st.task_id = wi.id \
         WHERE st.sprint_id = ? \
           AND wi.lane = ? \
           AND wi.status = 'todo' \
           AND wi.deleted_at IS NULL";

    let plan_rows = sqlx::query(CANDIDATE_EXPLAIN_SQL)
        .bind("sp-1")
        .bind("implement")
        .fetch_all(&pool)
        .await
        .expect("EXPLAIN QUERY PLAN on the claim-candidate SELECT");

    // The `detail` column (4th) of each EXPLAIN QUERY PLAN row carries the
    // human-readable plan step text.
    let plan: Vec<String> = plan_rows
        .iter()
        .map(|r| r.get::<String, _>("detail"))
        .collect();
    let plan_text = plan.join("\n");

    // The claim hot-path index must be used to SEARCH work_items.
    assert!(
        plan_text.contains("idx_work_items_claim"),
        "expected the claim partial index `idx_work_items_claim` in the query plan, got:\n{plan_text}"
    );
    // And work_items must NOT be a full table SCAN.
    assert!(
        !plan_text.contains("SCAN work_items") && !plan_text.contains("SCAN wi"),
        "expected work_items to be SEARCHed via the partial index, not full-SCANned; plan:\n{plan_text}"
    );
}

/// Build the legal hierarchy chain project→epic→focus→story→task, a sprint, and
/// a sprint_tasks membership row, so the claim-candidate join has a real shape.
/// The `focus` row carries a `shape` (required by the migration-0010 focus-shape
/// trigger); other kinds leave it NULL.
async fn seed_task_in_sprint(pool: &SqlitePool) {
    async fn insert_item(pool: &SqlitePool, id: &str, kind: &str, parent: Option<&str>) {
        let shape: Option<&str> = if kind == "focus" {
            Some("vertical-slice")
        } else {
            None
        };
        sqlx::query(
            "INSERT INTO work_items (id, kind, parent_id, title, status, shape, lane) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(kind)
        .bind(parent)
        .bind("title")
        .bind("todo")
        // Only the task carries a lane so it is a claim candidate; the
        // ancestors stay NULL-lane (not team-managed).
        .bind(shape)
        .bind(if kind == "task" { Some("implement") } else { None })
        .execute(pool)
        .await
        .unwrap_or_else(|e| panic!("seed {kind} {id}: {e}"));
    }

    insert_item(pool, "e-claim", "epic", Some("p-claim")).await;
    insert_item(pool, "fo-claim", "focus", Some("e-claim")).await;
    insert_item(pool, "s-claim", "story", Some("fo-claim")).await;
    insert_item(pool, "t-claim", "task", Some("s-claim")).await;

    // A sprint + membership row (migration-0011 tables).
    sqlx::query("INSERT INTO sprints (id, title, status) VALUES (?, ?, ?)")
        .bind("sp-1")
        .bind("claim sprint")
        .bind("open")
        .execute(pool)
        .await
        .expect("insert sprint");
    sqlx::query("INSERT INTO sprint_tasks (sprint_id, task_id) VALUES (?, ?)")
        .bind("sp-1")
        .bind("t-claim")
        .execute(pool)
        .await
        .expect("insert sprint_tasks membership");
}
