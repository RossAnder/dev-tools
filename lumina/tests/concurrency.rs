//! Concurrency smoke test for the sqlx-0.9 `BEGIN IMMEDIATE` adoption.
//!
//! Exercises the real SQLite lock manager (on-disk DB, WAL + 5s busy_timeout
//! enabled by `db::init`) under N=8 concurrent writers issuing
//! `repo::create_work_item` against the same pool. Each writer goes through
//! `db::begin_write` (a.k.a. `BEGIN IMMEDIATE`), so the writer lock is taken
//! at begin-time and the busy-handler serialises the eight tasks without
//! surfacing `SQLITE_BUSY`. The test would intermittently flake under the
//! previous DEFERRED-begin path: a task upgrades from read to write between
//! another task's begin and its first INSERT, the second writer's upgrade
//! fails, and the commit returns BUSY.
//!
//! In-memory DBs cannot exercise this path (no file → no real shared-cache
//! lock manager in default mode), so the test uses a `tempfile::TempDir` for
//! a real on-disk database that lives for the test's duration and is
//! recursively deleted on drop.

use std::sync::Arc;
use std::time::{Duration, Instant};

use lumina::{db, repo};
use sqlx::SqlitePool;
use tokio::task::JoinSet;

const CONCURRENT_WRITERS: usize = 8;

/// Open the on-disk SQLite pool used by the concurrency tests. WAL + the 5s
/// busy_timeout are enabled by `db::init` (the `is_in_memory` gate evaluates
/// to false for a tempdir path).
async fn open_on_disk_pool() -> (tempfile::TempDir, SqlitePool) {
    let tmp = tempfile::tempdir().expect("create tempdir for on-disk SQLite pool");
    let db_path = tmp.path().join("concurrency.db");
    let url = db_path.to_string_lossy().into_owned();
    let pool = db::init(&url).await.expect("init on-disk pool");
    (tmp, pool)
}

/// Eight tasks each create one `project` work-item concurrently against a
/// shared on-disk pool. With `BEGIN IMMEDIATE` + WAL + a 5s busy_timeout,
/// every writer eventually acquires the RESERVED lock; the post-condition is
/// that all eight rows landed and no task returned `SQLITE_BUSY`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_create_work_item_serialises_without_busy_error() {
    let (_tmp, pool) = open_on_disk_pool().await;
    let pool = Arc::new(pool);

    let start = Instant::now();

    let mut tasks = JoinSet::new();
    for i in 0..CONCURRENT_WRITERS {
        let pool = Arc::clone(&pool);
        tasks.spawn(async move {
            let title = format!("Concurrent Project {i}");
            repo::create_work_item(&pool, "project", None, &title, None).await
        });
    }

    let mut successes = 0usize;
    while let Some(joined) = tasks.join_next().await {
        let result = joined.expect("task panicked");
        result.unwrap_or_else(|e| panic!("create_work_item failed under contention: {e}"));
        successes += 1;
    }

    let elapsed = start.elapsed();

    assert_eq!(
        successes, CONCURRENT_WRITERS,
        "expected every concurrent writer to succeed",
    );

    // Two-second budget: each `create_work_item` is ~1ms uncontended;
    // serialised through the writer lock, eight of them should finish in tens
    // of milliseconds even on a cold tempdir. A regression that re-introduces
    // the DEFERRED-upgrade path would either fail above (BUSY) or stall here
    // (lock-contention slowdown), so the budget catches the silent-latency
    // failure mode that BUSY-handling alone hides.
    assert!(
        elapsed < Duration::from_secs(2),
        "eight serialised inserts should finish well under 2s, took {elapsed:?} — \
         likely lock-contention regression",
    );

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM work_items WHERE kind = 'project'")
        .fetch_one(&*pool)
        .await
        .expect("count projects");
    assert_eq!(
        count, CONCURRENT_WRITERS as i64,
        "every concurrent INSERT should be visible after all tasks join",
    );

    // Sanity-check the event outbox: the single-mutation-path discipline
    // means every successful `create_work_item` emits exactly one event row.
    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events WHERE aggregate_type = 'work_item' AND event_type = 'work_item.created'",
    )
    .fetch_one(&*pool)
    .await
    .expect("count work_item.created events");
    assert_eq!(
        events, CONCURRENT_WRITERS as i64,
        "every concurrent create should leave exactly one matching event",
    );
}
