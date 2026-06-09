//! Migration 0011 acceptance test (runs/sprints/findings-queue, schema-only).
//!
//! Proves the purely-additive migration applies cleanly to a fresh,
//! fully-migrated in-memory DB and that the new schema objects exist and behave
//! as designed:
//!   (a) The migration applies cleanly — `connect_in_memory` (which runs every
//!       embedded migration including 0011) succeeds.
//!   (b) Each new table exists: runs / sprints / sprint_tasks /
//!       finding_decisions.
//!   (c) The new columns are present: `run_id` + `triage_state` on `findings`,
//!       `spawned_from_finding_id` on `work_items`.
//!   (d) The partial unique index `ux_findings_dedup` exists.
//!   (e) The `runs` CHECKs are enforced (bogus kind / target_kind / status are
//!       rejected; a fully-valid row inserts), and `finding_decisions.decision`
//!       rejects a bogus disposition.
//!   (f) The `ux_findings_dedup` partial index actually dedups: a second LIVE
//!       finding with the SAME `(work_item_id, dedup_id)` and
//!       `superseded_by IS NULL` fails, while findings with `dedup_id IS NULL`
//!       are exempt (the partial predicate excludes them). This is the
//!       load-bearing assertion for the B17a dedup path.
//!
//! The pool comes from `db::connect_in_memory`, which enables
//! `foreign_keys(true)` per-connection. All assertions use the RUNTIME
//! `sqlx::query` / `query_scalar` string API (NOT compile-checked macros), so
//! this test introduces no `.sqlx/` cache entry — matches `migration_0010.rs`.

use lumina::db::connect_in_memory;
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

/// Raw INSERT of a work_item via the runtime query API (the hierarchy trigger
/// requires a `project` to have a NULL parent; a `task` chain is not needed for
/// these assertions — a single project row satisfies the FK targets used here).
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

/// Raw INSERT of a finding with an explicit `dedup_id` (NULL or a value) and an
/// explicit `superseded_by` (always NULL here — all inserted findings are LIVE).
async fn insert_finding(
    pool: &SqlitePool,
    id: &str,
    work_item_id: &str,
    dedup_id: Option<&str>,
) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
    sqlx::query("INSERT INTO findings (id, work_item_id, dedup_id) VALUES (?, ?, ?)")
        .bind(id)
        .bind(work_item_id)
        .bind(dedup_id)
        .execute(pool)
        .await
}

#[tokio::test]
async fn migration_0011_runs_sprints_findings_queue() {
    // (a) The whole embedded migration set (through 0011) applies on a fresh
    //     in-memory DB.
    let pool = connect_in_memory()
        .await
        .expect("migrated in-memory pool (0011 applied cleanly)");

    // (b) Each new table exists.
    for table in ["runs", "sprints", "sprint_tasks", "finding_decisions"] {
        assert_eq!(
            master_count(&pool, "table", table).await,
            1,
            "migration-0011 table `{table}` is present"
        );
    }

    // (c) The new columns are present.
    assert_eq!(
        shape_col(&pool, "findings", "run_id").await,
        1,
        "`findings.run_id` column is present"
    );
    assert_eq!(
        shape_col(&pool, "findings", "triage_state").await,
        1,
        "`findings.triage_state` column is present"
    );
    assert_eq!(
        shape_col(&pool, "work_items", "spawned_from_finding_id").await,
        1,
        "`work_items.spawned_from_finding_id` column is present"
    );

    // (d) The partial unique dedup index exists.
    assert_eq!(
        master_count(&pool, "index", "ux_findings_dedup").await,
        1,
        "the `ux_findings_dedup` partial unique index is present"
    );

    // (e) The `runs` CHECKs are enforced. These need no FK rows (runs.target_id
    //     is plain TEXT with no FK), so they exercise the CHECK directly.
    let bad_kind = sqlx::query(
        "INSERT INTO runs (id, kind, target_id, target_kind, status) VALUES (?, ?, ?, ?, ?)",
    )
    .bind("r-badkind")
    .bind("bogus")
    .bind("t1")
    .bind("story")
    .bind("open")
    .execute(&pool)
    .await;
    assert!(
        bad_kind.is_err(),
        "the runs.kind CHECK rejects a value outside review/optimise"
    );

    let bad_target = sqlx::query(
        "INSERT INTO runs (id, kind, target_id, target_kind, status) VALUES (?, ?, ?, ?, ?)",
    )
    .bind("r-badtarget")
    .bind("review")
    .bind("t1")
    .bind("bogus")
    .bind("open")
    .execute(&pool)
    .await;
    assert!(
        bad_target.is_err(),
        "the runs.target_kind CHECK rejects a value outside sprint/story"
    );

    let bad_status = sqlx::query(
        "INSERT INTO runs (id, kind, target_id, target_kind, status) VALUES (?, ?, ?, ?, ?)",
    )
    .bind("r-badstatus")
    .bind("review")
    .bind("t1")
    .bind("story")
    .bind("bogus")
    .execute(&pool)
    .await;
    assert!(
        bad_status.is_err(),
        "the runs.status CHECK rejects a value outside open/triaged/closed"
    );

    // A fully-valid runs row inserts cleanly.
    sqlx::query(
        "INSERT INTO runs (id, kind, target_id, target_kind, status) VALUES (?, ?, ?, ?, ?)",
    )
    .bind("r1")
    .bind("review")
    .bind("t1")
    .bind("story")
    .bind("open")
    .execute(&pool)
    .await
    .expect("a valid runs row inserts through all three CHECKs");

    // The `finding_decisions.decision` CHECK rejects a bogus disposition. We
    // satisfy the NOT-NULL `finding_id` FK by inserting a parent work_item +
    // finding first.
    insert_project(&pool, "p1")
        .await
        .expect("legal project under NULL parent");
    insert_finding(&pool, "fd-parent", "p1", None)
        .await
        .expect("a finding with NULL dedup_id inserts");

    let bad_decision = sqlx::query(
        "INSERT INTO finding_decisions (id, finding_id, decision) VALUES (?, ?, ?)",
    )
    .bind("d-bad")
    .bind("fd-parent")
    .bind("bogus")
    .execute(&pool)
    .await;
    assert!(
        bad_decision.is_err(),
        "the finding_decisions.decision CHECK rejects an out-of-vocabulary disposition"
    );

    // A valid finding_decisions row inserts (proving the CHECK admits the
    // documented vocabulary).
    sqlx::query("INSERT INTO finding_decisions (id, finding_id, decision) VALUES (?, ?, ?)")
        .bind("d-ok")
        .bind("fd-parent")
        .bind("spawn_task")
        .execute(&pool)
        .await
        .expect("a valid finding_decisions disposition inserts");

    // (f) The dedup partial index actually dedups. First LIVE finding with a
    //     non-NULL dedup_id on p1 inserts; a SECOND with the SAME
    //     (work_item_id, dedup_id) and superseded_by IS NULL must FAIL.
    insert_finding(&pool, "f-dup-1", "p1", Some("dd-1"))
        .await
        .expect("first LIVE finding with dedup_id dd-1 inserts");
    let dup = insert_finding(&pool, "f-dup-2", "p1", Some("dd-1")).await;
    assert!(
        dup.is_err(),
        "the ux_findings_dedup index rejects a second LIVE finding with the same (work_item_id, dedup_id)"
    );

    // A finding with the SAME dedup_id but on a DIFFERENT work_item is allowed
    // (the index keys on the pair). Insert a second project to prove it.
    insert_project(&pool, "p2")
        .await
        .expect("legal second project");
    insert_finding(&pool, "f-dup-3", "p2", Some("dd-1"))
        .await
        .expect("same dedup_id on a different work_item is allowed (pair-keyed index)");

    // findings with `dedup_id IS NULL` are EXEMPT from the partial index — two
    // NULL-dedup_id findings on the SAME work_item both insert OK (the WHERE
    // `dedup_id IS NOT NULL` predicate excludes them). fd-parent already covers
    // one NULL-dedup_id finding on p1; add a second.
    insert_finding(&pool, "f-null-2", "p1", None)
        .await
        .expect("a second NULL-dedup_id finding on the same work_item is exempt from the dedup index");

    // A SUPERSEDED finding is EXEMPT from the partial index — it exercises the
    // `superseded_by IS NULL` half of the predicate. Insert a finding with a
    // dedup tuple on a fresh project, mark it superseded (point superseded_by at
    // a live finding to satisfy the self-FK), then insert ANOTHER finding with
    // the SAME (work_item_id, dedup_id): it must SUCCEED, because the index only
    // constrains rows where superseded_by IS NULL — proving a legitimate re-flag
    // after supersession is allowed.
    insert_project(&pool, "p3")
        .await
        .expect("legal third project");
    insert_finding(&pool, "f-sup-1", "p3", Some("dd-2"))
        .await
        .expect("first finding with dedup_id dd-2 inserts");
    sqlx::query("UPDATE findings SET superseded_by = ? WHERE id = ?")
        .bind("f-dup-1")
        .bind("f-sup-1")
        .execute(&pool)
        .await
        .expect("mark f-sup-1 as superseded (self-FK to a live finding)");
    insert_finding(&pool, "f-sup-2", "p3", Some("dd-2"))
        .await
        .expect("a re-flag with the same (work_item_id, dedup_id) is allowed once the prior row is superseded (superseded_by IS NULL predicate)");
}
