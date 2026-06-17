//! Migration 0025 acceptance test — neutralise `reviews_work_item_id` +
//! reconcile in-flight OLD-MODEL separate review tasks (story 1B-F9, AC7).
//!
//! Migration 0025 makes `work_items.reviews_work_item_id` write-dead (no
//! `DROP COLUMN`) and, at apply time, reconciles any review task left over from
//! the migration-0013 separate-review-task model so it cannot hang its sprint as
//! a non-terminal orphan. The reconcile only matters for rows that ALREADY exist
//! when 0025 runs, so a plain `connect_in_memory` (which applies the whole
//! embedded set in one shot, including 0025, against an empty DB) cannot exercise
//! it. Instead we drive the REAL embedded migrator in two steps:
//!   1. `run_to(24, &pool)` — apply migrations 0001..=0024 only.
//!   2. Seed a legal hierarchy chain + an OLD-MODEL review task (lane='review',
//!      `reviews_work_item_id` set, status='in_progress' — non-terminal) bound
//!      into a sprint with the same `review -> impl` 'sequence' dependency edge
//!      the old `complete_task` cascade wrote.
//!   3. `run(&pool)` — apply the one remaining migration, 0025.
//!
//! Proves (AC7): after 0025 applies, NO non-terminal old-model review orphan
//! remains — the seeded review row is `cancelled`, and the dangling dependency
//! edge that touched it has been neutralised.
//!
//! All assertions use the RUNTIME `sqlx::query` / `query_scalar` string API (no
//! compile-checked macros), matching the other `migration_*.rs` tests.

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;

/// The embedded migration set, resolved at compile time relative to the crate
/// root (`lumina/core`), so `./migrations` is `lumina/core/migrations` — the
/// same directory `db::init` embeds via `sqlx::migrate!`.
static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Stand up a bare in-memory pool with foreign-key enforcement on (matching
/// `db::init`), WITHOUT running any migration — the caller drives `MIGRATOR`
/// step by step so it can seed between versions.
async fn bare_in_memory_pool() -> SqlitePool {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")
        .expect("parse in-memory url")
        .foreign_keys(true);
    SqlitePoolOptions::new()
        .max_connections(1) // one connection keeps the single :memory: db alive across calls
        .connect_with(opts)
        .await
        .expect("connect in-memory pool")
}

/// Insert one `work_item` row with explicit kind/parent/status (+ a shape for a
/// `focus`, required by the migration-0010 focus-shape trigger). Static SQL +
/// bound args (sqlx 0.9's `SqlSafeStr` bound rejects a dynamically-built
/// `&String`).
async fn insert_item(pool: &SqlitePool, id: &str, kind: &str, parent: Option<&str>, status: &str) {
    let shape: Option<&str> = if kind == "focus" {
        Some("vertical-slice")
    } else {
        None
    };
    sqlx::query(
        "INSERT INTO work_items (id, kind, parent_id, title, status, shape) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(kind)
    .bind(parent)
    .bind("title")
    .bind(status)
    .bind(shape)
    .execute(pool)
    .await
    .unwrap_or_else(|e| panic!("seed {kind} {id}: {e}"));
}

#[tokio::test]
async fn migration_0025_cancels_inflight_old_model_review_orphans() {
    let pool = bare_in_memory_pool().await;

    // (1) Apply migrations 0001..=0024 only — 0025 stays pending so we can seed
    //     an old-model review row that PRE-DATES the reconcile.
    MIGRATOR
        .run_to(24, &pool)
        .await
        .expect("apply migrations through 0024");

    // (2) Seed the legal hierarchy chain project -> epic -> focus -> story -> task
    //     (the implement task), plus an OLD-MODEL review task under the same
    //     story, exactly as the migration-0013 `complete_task` cascade left it.
    insert_item(&pool, "p-1", "project", None, "active").await;
    insert_item(&pool, "e-1", "epic", Some("p-1"), "active").await;
    insert_item(&pool, "fo-1", "focus", Some("e-1"), "active").await;
    insert_item(&pool, "s-1", "story", Some("fo-1"), "active").await;
    // The implementation task (already done — the review covers it).
    insert_item(&pool, "impl-1", "task", Some("s-1"), "done").await;
    // The OLD-MODEL review task: parent = the story, lane='review', a
    // `reviews_work_item_id` back-link to the impl task, NON-TERMINAL status.
    insert_item(&pool, "rev-1", "task", Some("s-1"), "in_progress").await;
    sqlx::query(
        "UPDATE work_items SET lane = 'review', reviews_work_item_id = 'impl-1', tier = NULL \
         WHERE id = 'rev-1'",
    )
    .execute(&pool)
    .await
    .expect("stamp old-model review back-link");

    // The `review -> impl` 'sequence' dependency edge the cascade wrote.
    sqlx::query(
        "INSERT INTO task_dependencies (task_id, depends_on_id, kind) \
         VALUES ('rev-1', 'impl-1', 'sequence')",
    )
    .execute(&pool)
    .await
    .expect("seed review->impl dependency edge");

    // A sprint + membership rows so the review task is genuinely in a sprint's
    // queue (the orphan it would otherwise become).
    sqlx::query("INSERT INTO sprints (id, title, status) VALUES ('sp-1', 'sprint', 'active')")
        .execute(&pool)
        .await
        .expect("insert sprint");
    for task in ["impl-1", "rev-1"] {
        sqlx::query("INSERT INTO sprint_tasks (sprint_id, task_id) VALUES ('sp-1', ?)")
            .bind(task)
            .execute(&pool)
            .await
            .expect("insert sprint membership");
    }

    // Sanity: before 0025 the review row is a live non-terminal orphan.
    let pre: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM work_items \
         WHERE lane = 'review' AND reviews_work_item_id IS NOT NULL \
           AND status NOT IN ('done', 'cancelled') AND deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("count pre-reconcile orphans");
    assert_eq!(pre, 1, "the seeded old-model review task is a live non-terminal orphan before 0025");

    // (3) Apply the remaining migration (0025) — its reconcile runs now, against
    //     the seeded row.
    MIGRATOR
        .run(&pool)
        .await
        .expect("apply migration 0025");

    // AC7: no non-terminal old-model review orphan remains.
    let post: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM work_items \
         WHERE lane = 'review' AND reviews_work_item_id IS NOT NULL \
           AND status NOT IN ('done', 'cancelled') AND deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("count post-reconcile orphans");
    assert_eq!(post, 0, "AC7: migration 0025 leaves no non-terminal old-model review orphan");

    // The seeded review row was specifically cancelled (not deleted, not done).
    let rev_status: String = sqlx::query_scalar("SELECT status FROM work_items WHERE id = 'rev-1'")
        .fetch_one(&pool)
        .await
        .expect("read review status");
    assert_eq!(rev_status, "cancelled", "the in-flight review task is cancelled");

    // The dangling `review -> impl` dependency edge was neutralised.
    let edges: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_dependencies WHERE task_id = 'rev-1' OR depends_on_id = 'rev-1'",
    )
    .fetch_one(&pool)
    .await
    .expect("count residual review edges");
    assert_eq!(edges, 0, "the old-model review dependency edge is removed");

    // The implementation task is untouched by the reconcile (still done).
    let impl_status: String =
        sqlx::query_scalar("SELECT status FROM work_items WHERE id = 'impl-1'")
            .fetch_one(&pool)
            .await
            .expect("read impl status");
    assert_eq!(impl_status, "done", "the reconcile does not touch the implementation task");
}
