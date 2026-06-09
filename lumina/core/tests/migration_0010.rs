//! Migration 0010 acceptance test (epic/focus semantics, schema half).
//!
//! Proves the `feature`→`focus` rename + `shape` column migration applies
//! cleanly to a fresh, fully-migrated in-memory DB and that the recreated
//! hierarchy-guard triggers + the new `shape` CHECK behave as designed:
//!   (a) The migration applies cleanly — `connect_in_memory` (which runs every
//!       embedded migration including 0010) succeeds and the `shape` column is
//!       present on `work_items`.
//!   (b) A LEGAL project→epic→focus→story chain inserts via the recreated
//!       BEFORE INSERT hierarchy trigger (the epic→child edge now references
//!       'focus', not 'feature').
//!   (c) The renamed trigger still ABORTS illegal edges: the legacy 'feature'
//!       kind under an epic is rejected (the kind no longer appears in the
//!       trigger's allow-list), and a 'story' directly under an epic is rejected
//!       (a story's only legal parent is a 'focus').
//!   (d) The `shape` CHECK rejects an out-of-vocabulary value (a focus row whose
//!       shape is neither NULL nor one of vertical-slice/cross-cutting/
//!       foundational).
//!   (g) The focus-shape guard triggers (R19) reject a focus with NULL shape on
//!       both INSERT and UPDATE — the focus-must-have-shape invariant now lives
//!       at the DB layer, not only in the repo create/update path.
//!
//! The pool comes from `db::connect_in_memory`, which enables
//! `foreign_keys(true)` per-connection. All assertions use the RUNTIME
//! `sqlx::query` / `query_scalar` string API (NOT compile-checked macros), so
//! this test introduces no `.sqlx/` cache entry — matches `migration_0003.rs` /
//! `migration_0004.rs`.

use lumina_core::db::connect_in_memory;
use sqlx::SqlitePool;

/// Raw INSERT of a work_item via the runtime query API. Returns the execute
/// result so callers can assert success / trigger-ABORT. `shape` is bound
/// explicitly so the focus rows + the CHECK-violation case can exercise it.
async fn insert_item(
    pool: &SqlitePool,
    id: &str,
    kind: &str,
    parent: Option<&str>,
    shape: Option<&str>,
) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
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
}

#[tokio::test]
async fn migration_0010_applies_and_renames_feature_to_focus() {
    // (a) The whole embedded migration set (through 0010) applies on a fresh
    //     in-memory DB.
    let pool = connect_in_memory()
        .await
        .expect("migrated in-memory pool (0010 applied cleanly)");

    // The `shape` column exists on work_items (added by 0010).
    let shape_col: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('work_items') WHERE name = 'shape'",
    )
    .fetch_one(&pool)
    .await
    .expect("inspect work_items columns");
    assert_eq!(shape_col, 1, "the migration-0010 `shape` column is present");

    // (b) A LEGAL project→epic→focus→story chain inserts through the recreated
    //     hierarchy trigger (the focus level is the renamed feature level).
    insert_item(&pool, "p1", "project", None, None)
        .await
        .expect("legal project under NULL parent");
    insert_item(&pool, "e1", "epic", Some("p1"), None)
        .await
        .expect("legal epic under project");
    insert_item(&pool, "f1", "focus", Some("e1"), Some("vertical-slice"))
        .await
        .expect("legal focus under epic");
    insert_item(&pool, "s1", "story", Some("f1"), None)
        .await
        .expect("legal story under focus");

    // (c) The recreated trigger ABORTS illegal edges.
    //     The legacy 'feature' kind is no longer in the trigger allow-list.
    let legacy_feature = insert_item(&pool, "x1", "feature", Some("e1"), None).await;
    assert!(
        legacy_feature.is_err(),
        "the recreated hierarchy trigger rejects the legacy 'feature' kind under an epic"
    );
    //     A story directly under an epic is illegal (a story's only legal parent
    //     is a focus).
    let story_under_epic = insert_item(&pool, "x2", "story", Some("e1"), None).await;
    assert!(
        story_under_epic.is_err(),
        "the recreated hierarchy trigger rejects a story directly under an epic"
    );

    // (d) The `shape` CHECK rejects an out-of-vocabulary value.
    let bad_shape = insert_item(&pool, "x3", "focus", Some("e1"), Some("not-a-shape")).await;
    assert!(
        bad_shape.is_err(),
        "the `shape` CHECK rejects a value outside vertical-slice/cross-cutting/foundational"
    );

    // (d') The other two valid shape values INSERT cleanly through the CHECK
    //      (R9: the CHECK was previously exercised for only one valid value,
    //      `vertical-slice`, plus one rejected value).
    insert_item(&pool, "f2", "focus", Some("e1"), Some("cross-cutting"))
        .await
        .expect("the `shape` CHECK accepts cross-cutting");
    insert_item(&pool, "f3", "focus", Some("e1"), Some("foundational"))
        .await
        .expect("the `shape` CHECK accepts foundational");

    // (g) The focus-shape guard (R19): a focus row with NULL shape is REJECTED
    //     by the BEFORE INSERT trigger regardless of writer. The `shape` CHECK
    //     itself permits NULL for all kinds (so f4 would pass the CHECK), but the
    //     trigger moves the focus-must-have-shape invariant to the DB layer.
    //     Contrast: a focus WITH a shape inserts OK — proven by f1/f2/f3 above.
    let focus_null_shape_insert = insert_item(&pool, "f4", "focus", Some("e1"), None).await;
    assert!(
        focus_null_shape_insert.is_err(),
        "the focus-shape guard trigger rejects a focus inserted with NULL shape"
    );

    // (g') The UPDATE twin of the focus-shape guard rejects nulling out an
    //      existing focus's shape. f1 was inserted with 'vertical-slice'; setting
    //      its shape to NULL must ABORT.
    let focus_null_shape_update = sqlx::query("UPDATE work_items SET shape = NULL WHERE id = ?")
        .bind("f1")
        .execute(&pool)
        .await;
    assert!(
        focus_null_shape_update.is_err(),
        "the focus-shape guard UPDATE trigger rejects nulling an existing focus's shape"
    );

    // (e) A focus under a NON-epic parent is rejected — proves the INSERT
    //     allow-list narrowed to exactly epic→focus (R28a). A focus directly
    //     under a project is illegal.
    let focus_under_project = insert_item(&pool, "x4", "focus", Some("p1"), Some("vertical-slice")).await;
    assert!(
        focus_under_project.is_err(),
        "the hierarchy trigger rejects a focus directly under a project (a focus's only legal parent is an epic)"
    );

    // (f) The UPDATE-trigger twin (trg_work_items_hierarchy_update) ABORTS an
    //     illegal parent edge introduced by an UPDATE (R28b: only the INSERT
    //     trigger was previously exercised). Reparent the legal story s1 (focus→
    //     story) onto the epic e1, which is an illegal story→epic edge.
    let illegal_reparent = sqlx::query("UPDATE work_items SET parent_id = ? WHERE id = ?")
        .bind("e1")
        .bind("s1")
        .execute(&pool)
        .await;
    assert!(
        illegal_reparent.is_err(),
        "the UPDATE hierarchy trigger rejects reparenting a story directly under an epic"
    );
}
