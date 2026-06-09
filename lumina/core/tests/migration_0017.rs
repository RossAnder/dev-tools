//! Migration 0017 acceptance test — `work_items.checkpoint` 0/1 write-time guard
//! (review R12 follow-up to migration 0016).
//!
//! Migration 0016 added `work_items.checkpoint` as a bare nullable INTEGER with
//! no column-level CHECK (SQLite cannot `ALTER TABLE ADD CONSTRAINT`). The
//! claim-time checkpoint-freeze barrier keys strictly on `checkpoint = 1`, so a
//! stray out-of-band value would silently disable the freeze. Migration 0017
//! installs a BEFORE INSERT / BEFORE UPDATE trigger pair (mirroring the existing
//! `trg_work_items_attributes_*` validation idiom) that rejects any non-NULL
//! `checkpoint` outside `(0, 1)`.
//!
//! Proves:
//!   (a) The migration applies cleanly to a fresh fully-migrated in-memory DB
//!       (`connect_in_memory` runs every embedded migration incl. 0017).
//!   (b) Both validation triggers exist.
//!   (c) An out-of-band `checkpoint` value is rejected on INSERT and on UPDATE,
//!       while NULL / 0 / 1 succeed — so the freeze barrier can never be
//!       silently disabled by a stray value, and a rejected UPDATE leaves the
//!       prior value intact (statement aborted atomically).
//!
//! All assertions use the RUNTIME `sqlx::query` / `query_scalar` string API (no
//! compile-checked macros), matching `migration_0016.rs`.

use lumina_core::db::connect_in_memory;
use sqlx::SqlitePool;

/// COUNT of `sqlite_master` rows of a given `type`/`name` — 1 when the object
/// exists.
async fn master_count(pool: &SqlitePool, kind: &str, name: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = ? AND name = ?")
        .bind(kind)
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("inspect sqlite_master")
}

/// Insert a minimal valid `project` row (NULL parent — permitted by the
/// hierarchy trigger) with a bound `checkpoint` value (`None` → NULL), returning
/// the `Result` so the caller can assert ok/err. The SQL stays a `&'static str`
/// literal — sqlx 0.9's `SqlSafeStr` bound rejects a dynamically-built `&String`
/// (see `migration_0016.rs`), so the value rides a bind parameter, not splicing.
async fn insert_project(
    pool: &SqlitePool,
    id: &str,
    checkpoint: Option<i64>,
) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO work_items (id, kind, parent_id, title, status, checkpoint) \
         VALUES (?, 'project', NULL, 'P', 'todo', ?)",
    )
    .bind(id)
    .bind(checkpoint)
    .execute(pool)
    .await
}

#[tokio::test]
async fn migration_0017_applies_and_triggers_exist() {
    let pool = connect_in_memory().await.expect("pool");
    assert_eq!(
        master_count(&pool, "trigger", "trg_work_items_checkpoint_insert").await,
        1,
        "the BEFORE INSERT checkpoint guard trigger exists"
    );
    assert_eq!(
        master_count(&pool, "trigger", "trg_work_items_checkpoint_update").await,
        1,
        "the BEFORE UPDATE checkpoint guard trigger exists"
    );
}

#[tokio::test]
async fn checkpoint_insert_rejects_out_of_band_value() {
    let pool = connect_in_memory().await.expect("pool");

    // NULL / 0 / 1 are all legal.
    insert_project(&pool, "p-null", None).await.expect("checkpoint NULL is legal");
    insert_project(&pool, "p-zero", Some(0)).await.expect("checkpoint 0 is legal");
    insert_project(&pool, "p-one", Some(1)).await.expect("checkpoint 1 is legal");

    // 2 (and any other out-of-band value) is rejected by the BEFORE INSERT trigger.
    let res = insert_project(&pool, "p-bad", Some(2)).await;
    assert!(res.is_err(), "checkpoint = 2 must be rejected on INSERT, got {res:?}");

    // The rejected row never landed.
    let present: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM work_items WHERE id = 'p-bad'")
        .fetch_one(&pool)
        .await
        .expect("count p-bad");
    assert_eq!(present, 0, "the aborted INSERT persisted no row");
}

#[tokio::test]
async fn checkpoint_update_rejects_out_of_band_value_and_preserves_prior() {
    let pool = connect_in_memory().await.expect("pool");
    insert_project(&pool, "p1", None).await.expect("seed project");

    // Legal transitions succeed.
    sqlx::query("UPDATE work_items SET checkpoint = 1 WHERE id = 'p1'")
        .execute(&pool)
        .await
        .expect("checkpoint -> 1 is legal");
    sqlx::query("UPDATE work_items SET checkpoint = 0 WHERE id = 'p1'")
        .execute(&pool)
        .await
        .expect("checkpoint -> 0 is legal");

    // An out-of-band UPDATE is aborted.
    let bad = sqlx::query("UPDATE work_items SET checkpoint = 2 WHERE id = 'p1'")
        .execute(&pool)
        .await;
    assert!(bad.is_err(), "checkpoint -> 2 must be rejected on UPDATE, got {bad:?}");

    // The aborted UPDATE left the prior value (0) intact.
    let cp: Option<i64> = sqlx::query_scalar("SELECT checkpoint FROM work_items WHERE id = 'p1'")
        .fetch_one(&pool)
        .await
        .expect("read checkpoint");
    assert_eq!(cp, Some(0), "the rejected UPDATE did not mutate the row");

    // Clearing back to NULL stays legal.
    sqlx::query("UPDATE work_items SET checkpoint = NULL WHERE id = 'p1'")
        .execute(&pool)
        .await
        .expect("checkpoint -> NULL is legal");
}
