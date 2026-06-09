//! Migration 0004 acceptance test (project↔repo-links plan, Task T1).
//!
//! Proves the additive `repo_links` migration applies cleanly to a fresh,
//! fully-migrated in-memory DB and that the new structure behaves as designed:
//!   (a) `findings.repo_id` is a real FK — inserting a finding with a bogus
//!       `repo_id` returns a SQLite constraint error.
//!   (b) Inserting a finding without `repo_id` succeeds (the column defaults
//!       to NULL via ADD COLUMN, guarding the SQLite ALTER-ADD-with-REFERENCES
//!       pitfall flagged in the plan).
//!   (c) The `repo_links` kind-check trigger pair (BEFORE INSERT + BEFORE
//!       UPDATE) refuses a non-`project` parent — proves the BEFORE INSERT
//!       guard is wired AND the BEFORE UPDATE guard mirrors it.
//!   (d) The partial UNIQUE index `idx_repo_links_one_primary` enforces
//!       at-most-one primary per project (a second insert with is_primary=1
//!       under the same project errors).
//!   (e) Deleting a project cascade-deletes its `repo_links` rows AND
//!       `ON DELETE SET NULL` on `findings.repo_id` leaves the finding intact
//!       with `repo_id IS NULL` when the linked repo is removed.
//!
//! The pool comes from `db::connect_in_memory`, which enables
//! `foreign_keys(true)` per-connection. All assertions use the RUNTIME
//! `sqlx::query` / `query_scalar` string API (NOT compile-checked macros), so
//! this test introduces no `.sqlx/` cache entry — matches `migration_0003.rs`.

use lumina_core::db::connect_in_memory;
use sqlx::SqlitePool;

/// Seed the project→epic→focus→story chain and return the story id, so we
/// can attach findings to a real (legal) `task` parent. The hierarchy trigger
/// rejects findings on stray work_items, but findings have no kind-check —
/// they reference any work_item — so a story will do.
async fn seed_story(pool: &SqlitePool) -> String {
    // `shape` is bound for the focus row: migration 0010's focus-shape guard
    // trigger rejects a focus with NULL shape, so the seed must supply one.
    for (id, kind, parent, shape) in [
        ("p1", "project", None, None),
        ("e1", "epic", Some("p1"), None),
        ("f1", "focus", Some("e1"), Some("vertical-slice")),
        ("s1", "story", Some("f1"), None),
    ] {
        sqlx::query(
            "INSERT INTO work_items (id, kind, parent_id, title, status, shape) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(kind)
        .bind(parent)
        .bind(format!("{kind} title"))
        .bind("open")
        .bind(shape)
        .execute(pool)
        .await
        .unwrap_or_else(|e| panic!("seed {kind} {id}: {e}"));
    }
    "s1".to_string()
}

async fn count(pool: &SqlitePool, sql: &'static str) -> i64 {
    sqlx::query_scalar::<_, i64>(sql)
        .fetch_one(pool)
        .await
        .expect("count query")
}

#[tokio::test]
async fn migration_0004_findings_repo_id_fk_is_enforced() {
    let pool = connect_in_memory()
        .await
        .expect("migrated in-memory pool (0004 applied cleanly)");

    let story = seed_story(&pool).await;

    // (b) Inserting a finding WITHOUT repo_id succeeds — the new column
    // defaults to NULL (the ADD COLUMN REFERENCES pitfall from the plan: a
    // non-NULL default would have aborted the migration under
    // foreign_keys=ON).
    sqlx::query(
        "INSERT INTO findings (id, work_item_id, summary) VALUES (?, ?, ?)",
    )
    .bind("f-null-repo")
    .bind(&story)
    .bind("no-repo finding")
    .execute(&pool)
    .await
    .expect("insert finding without repo_id");

    let null_repo_id: Option<String> = sqlx::query_scalar::<_, Option<String>>(
        "SELECT repo_id FROM findings WHERE id = ?",
    )
    .bind("f-null-repo")
    .fetch_one(&pool)
    .await
    .expect("read back null repo_id");
    assert!(
        null_repo_id.is_none(),
        "ADD COLUMN repo_id should default to NULL"
    );

    // (a) Inserting a finding with a BOGUS repo_id errors — the FK is real.
    let res = sqlx::query(
        "INSERT INTO findings (id, work_item_id, summary, repo_id) VALUES (?, ?, ?, ?)",
    )
    .bind("f-bad-repo")
    .bind(&story)
    .bind("bad-repo finding")
    .bind("does-not-exist")
    .execute(&pool)
    .await;
    let err = res.expect_err("FK violation expected");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("FOREIGN KEY") || msg.contains("constraint"),
        "expected a foreign-key constraint error, got: {msg}"
    );
}

#[tokio::test]
async fn migration_0004_repo_links_kind_check_trigger_pair() {
    let pool = connect_in_memory()
        .await
        .expect("migrated in-memory pool (0004 applied cleanly)");

    let _story = seed_story(&pool).await;

    // A repo_links row pointing at a NON-project (the story) is refused by the
    // BEFORE INSERT trigger.
    let insert_bad = sqlx::query(
        "INSERT INTO repo_links (id, project_id, slug, position, is_primary, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind("rl-bad")
    .bind("s1") // a story, not a project
    .bind("octocat/hello-world")
    .bind(0_i64)
    .bind(1_i64)
    .bind("2026-05-25T00:00:00Z")
    .execute(&pool)
    .await;
    assert!(
        insert_bad.is_err(),
        "BEFORE INSERT trigger must reject a repo_links row whose project_id is not kind=project"
    );

    // Insert a legal repo_links row (project_id = p1 / kind=project).
    sqlx::query(
        "INSERT INTO repo_links (id, project_id, slug, position, is_primary, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind("rl1")
    .bind("p1")
    .bind("octocat/hello-world")
    .bind(0_i64)
    .bind(1_i64)
    .bind("2026-05-25T00:00:00Z")
    .execute(&pool)
    .await
    .expect("legal repo_links insert");

    // Now try to UPDATE the legal row to point at a non-project — the BEFORE
    // UPDATE trigger must mirror the INSERT trigger and refuse it.
    let update_bad = sqlx::query("UPDATE repo_links SET project_id = ? WHERE id = ?")
        .bind("s1") // a story, not a project
        .bind("rl1")
        .execute(&pool)
        .await;
    assert!(
        update_bad.is_err(),
        "BEFORE UPDATE trigger must mirror the BEFORE INSERT trigger"
    );
}

#[tokio::test]
async fn migration_0004_repo_links_one_primary_partial_unique_index() {
    let pool = connect_in_memory()
        .await
        .expect("migrated in-memory pool (0004 applied cleanly)");

    let _story = seed_story(&pool).await;

    // First primary link is legal.
    sqlx::query(
        "INSERT INTO repo_links (id, project_id, slug, position, is_primary, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind("rl-primary-1")
    .bind("p1")
    .bind("octocat/hello-world")
    .bind(0_i64)
    .bind(1_i64) // is_primary
    .bind("2026-05-25T00:00:00Z")
    .execute(&pool)
    .await
    .expect("first primary insert");

    // Second primary on the SAME project is rejected by the partial UNIQUE
    // index `idx_repo_links_one_primary`.
    let dup_primary = sqlx::query(
        "INSERT INTO repo_links (id, project_id, slug, position, is_primary, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind("rl-primary-2")
    .bind("p1")
    .bind("octocat/spoon-knife")
    .bind(1_i64)
    .bind(1_i64) // is_primary — duplicate under the partial index
    .bind("2026-05-25T00:00:00Z")
    .execute(&pool)
    .await;
    assert!(
        dup_primary.is_err(),
        "partial UNIQUE index must reject a second is_primary=1 under the same project"
    );

    // Inserting a non-primary link under the same project succeeds (the
    // partial index does not affect is_primary=0 rows).
    sqlx::query(
        "INSERT INTO repo_links (id, project_id, slug, position, is_primary, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind("rl-secondary")
    .bind("p1")
    .bind("octocat/spoon-knife")
    .bind(1_i64)
    .bind(0_i64) // not primary
    .bind("2026-05-25T00:00:00Z")
    .execute(&pool)
    .await
    .expect("non-primary insert is independent of the partial unique index");
}

#[tokio::test]
async fn migration_0004_cascade_and_set_null_paths() {
    let pool = connect_in_memory()
        .await
        .expect("migrated in-memory pool (0004 applied cleanly)");

    let story = seed_story(&pool).await;

    // Set up: one project with two repo_links; one finding bound to the
    // secondary repo_link.
    sqlx::query(
        "INSERT INTO repo_links (id, project_id, slug, position, is_primary, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind("rl-prim")
    .bind("p1")
    .bind("octocat/hello-world")
    .bind(0_i64)
    .bind(1_i64)
    .bind("2026-05-25T00:00:00Z")
    .execute(&pool)
    .await
    .expect("primary link");
    sqlx::query(
        "INSERT INTO repo_links (id, project_id, slug, position, is_primary, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind("rl-sec")
    .bind("p1")
    .bind("octocat/spoon-knife")
    .bind(1_i64)
    .bind(0_i64)
    .bind("2026-05-25T00:00:00Z")
    .execute(&pool)
    .await
    .expect("secondary link");

    sqlx::query(
        "INSERT INTO findings (id, work_item_id, summary, repo_id) VALUES (?, ?, ?, ?)",
    )
    .bind("f-bound")
    .bind(&story)
    .bind("bound to secondary repo")
    .bind("rl-sec")
    .execute(&pool)
    .await
    .expect("finding bound to secondary repo_link");

    // Delete the secondary repo_link — ON DELETE SET NULL on findings.repo_id
    // must clear the FK but keep the finding row.
    sqlx::query("DELETE FROM repo_links WHERE id = ?")
        .bind("rl-sec")
        .execute(&pool)
        .await
        .expect("delete secondary link");
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM findings WHERE id = 'f-bound'").await,
        1,
        "finding row must survive the linked-repo removal"
    );
    let cleared: Option<String> = sqlx::query_scalar::<_, Option<String>>(
        "SELECT repo_id FROM findings WHERE id = ?",
    )
    .bind("f-bound")
    .fetch_one(&pool)
    .await
    .expect("read finding repo_id");
    assert!(
        cleared.is_none(),
        "ON DELETE SET NULL must clear findings.repo_id"
    );

    // Cascade check: spin up a standalone project (no descendants — the
    // primary project p1 has epics/focuses/stories referencing it via a
    // self-FK that is NOT ON DELETE CASCADE, so the schema-shaped way to
    // observe the repo_links cascade is on a childless project). Attach a
    // repo_link to it, delete the project, and assert the link is gone.
    sqlx::query("INSERT INTO work_items (id, kind, parent_id, title, status) VALUES (?, ?, NULL, ?, ?)")
        .bind("p2-cascade")
        .bind("project")
        .bind("standalone")
        .bind("open")
        .execute(&pool)
        .await
        .expect("seed standalone project");
    sqlx::query(
        "INSERT INTO repo_links (id, project_id, slug, position, is_primary, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind("rl-cascade")
    .bind("p2-cascade")
    .bind("octocat/cascade-target")
    .bind(0_i64)
    .bind(0_i64)
    .bind("2026-05-25T00:00:00Z")
    .execute(&pool)
    .await
    .expect("link on standalone project");

    sqlx::query("DELETE FROM work_items WHERE id = ?")
        .bind("p2-cascade")
        .execute(&pool)
        .await
        .expect("delete standalone project");
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM repo_links WHERE project_id = 'p2-cascade'",
        )
        .await,
        0,
        "repo_links must cascade-delete with the owning project"
    );
}
