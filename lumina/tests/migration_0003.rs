//! Migration 0003 acceptance test (Plan 1.5, Task 1).
//!
//! Proves the additive planning/decision migration applies cleanly to a fresh,
//! fully-migrated in-memory DB and that the new FK child tables behave per the
//! 0002 idiom:
//!   (a) deleting a work_items row cascade-deletes its acceptance_criteria,
//!       research_notes, and open_questions rows;
//!   (b) deleting an open_questions row cascade-deletes its question_options;
//!   (c) UNIQUE(parent_id, seq) is enforced (a duplicate insert errors).
//!
//! The pool comes from `db::connect_in_memory`, which enables
//! `foreign_keys(true)` per-connection — the cascades only fire because of that
//! pool option, not the PRAGMA in the migration file. All assertions use the
//! RUNTIME `sqlx::query` / `query_scalar` string API (NOT the compile-checked
//! `query!` macros), so this test introduces no `.sqlx/` cache entry.
//!
//! Rows are created PARENT-FIRST (a research_note before the open_question that
//! references it via `prompting_note_id`; the open_question before its
//! question_options) so the insert-time FK check is genuinely exercised.

use lumina::db::connect_in_memory;
use sqlx::SqlitePool;

/// Build the legal project→epic→feature→story chain and return the story id.
/// The hierarchy trigger (0001) requires legal parentage; a story is the parent
/// for both acceptance_criteria/research_notes (we attach to the story here) and
/// open_questions (story-scoped).
async fn seed_story(pool: &SqlitePool) -> String {
    for (id, kind, parent) in [
        ("p1", "project", None),
        ("e1", "epic", Some("p1")),
        ("f1", "focus", Some("e1")),
        ("s1", "story", Some("f1")),
    ] {
        sqlx::query(
            "INSERT INTO work_items (id, kind, parent_id, title, status) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(kind)
        .bind(parent)
        .bind(format!("{kind} title"))
        .bind("open")
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
async fn migration_0003_applies_and_cascades_hold() {
    let pool = connect_in_memory()
        .await
        .expect("migrated in-memory pool (0003 applied cleanly)");

    let story = seed_story(&pool).await;

    // --- acceptance_criteria child (FK -> work_items, CASCADE) ---------------
    sqlx::query(
        "INSERT INTO acceptance_criteria (id, work_item_id, seq, text) \
         VALUES (?, ?, ?, ?)",
    )
    .bind("ac1")
    .bind(&story)
    .bind(1_i64)
    .bind("criterion one")
    .execute(&pool)
    .await
    .expect("insert acceptance_criterion");

    // --- research_note child (FK -> work_items, CASCADE) ---------------------
    // Created BEFORE the open_question that references it via prompting_note_id,
    // so the insert-time FK check on open_questions.prompting_note_id is real.
    sqlx::query(
        "INSERT INTO research_notes (id, work_item_id, seq, summary, state) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind("rn1")
    .bind(&story)
    .bind(1_i64)
    .bind("a research note")
    .bind("proposed")
    .execute(&pool)
    .await
    .expect("insert research_note");

    // --- open_question child (FK -> work_items story, CASCADE) ---------------
    // References the existing research_note via prompting_note_id (parent-first).
    sqlx::query(
        "INSERT INTO open_questions (id, story_id, seq, question, status, prompting_note_id) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind("oq1")
    .bind(&story)
    .bind(1_i64)
    .bind("which option?")
    .bind("open")
    .bind("rn1")
    .execute(&pool)
    .await
    .expect("insert open_question referencing existing research_note");

    // --- question_option child (FK -> open_questions, CASCADE) ---------------
    // Created AFTER its parent open_question (parent-first), exercising the
    // insert-time FK check on question_options.question_id.
    sqlx::query(
        "INSERT INTO question_options (id, question_id, seq, label) \
         VALUES (?, ?, ?, ?)",
    )
    .bind("qo1")
    .bind("oq1")
    .bind(1_i64)
    .bind("option A")
    .execute(&pool)
    .await
    .expect("insert question_option referencing existing open_question");

    // Pre-delete: every child row exists.
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM acceptance_criteria").await, 1);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM research_notes").await, 1);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM open_questions").await, 1);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM question_options").await, 1);

    // (c) UNIQUE(parent_id, seq) is enforced — a duplicate (work_item_id, seq)
    // on acceptance_criteria must error.
    let dup = sqlx::query(
        "INSERT INTO acceptance_criteria (id, work_item_id, seq, text) \
         VALUES (?, ?, ?, ?)",
    )
    .bind("ac1-dup")
    .bind(&story)
    .bind(1_i64) // same seq as ac1 under the same work_item
    .bind("duplicate seq")
    .execute(&pool)
    .await;
    assert!(
        dup.is_err(),
        "expected UNIQUE(work_item_id, seq) to reject a duplicate seq, got Ok"
    );

    // (c) UNIQUE(work_item_id, seq) is enforced on research_notes.
    let dup_rn = sqlx::query(
        "INSERT INTO research_notes (id, work_item_id, seq, summary, state) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind("rn1-dup")
    .bind(&story)
    .bind(1_i64) // same seq as rn1 under the same work_item
    .bind("duplicate seq research note")
    .bind("proposed")
    .execute(&pool)
    .await;
    assert!(
        dup_rn.is_err(),
        "expected UNIQUE(work_item_id, seq) on research_notes to reject a duplicate seq, got Ok"
    );

    // (c) UNIQUE(story_id, seq) is enforced on open_questions.
    let dup_oq = sqlx::query(
        "INSERT INTO open_questions (id, story_id, seq, question, status) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind("oq1-dup")
    .bind(&story)
    .bind(1_i64) // same seq as oq1 under the same story
    .bind("duplicate seq question")
    .bind("open")
    .execute(&pool)
    .await;
    assert!(
        dup_oq.is_err(),
        "expected UNIQUE(story_id, seq) on open_questions to reject a duplicate seq, got Ok"
    );

    // (c) UNIQUE(question_id, seq) is enforced on question_options.
    let dup_qo = sqlx::query(
        "INSERT INTO question_options (id, question_id, seq, label) \
         VALUES (?, ?, ?, ?)",
    )
    .bind("qo1-dup")
    .bind("oq1")
    .bind(1_i64) // same seq as qo1 under the same question
    .bind("duplicate seq option")
    .execute(&pool)
    .await;
    assert!(
        dup_qo.is_err(),
        "expected UNIQUE(question_id, seq) on question_options to reject a duplicate seq, got Ok"
    );

    // (b) Deleting the open_question cascade-deletes its question_options. Do
    // this BEFORE deleting the story so it is unambiguously the question delete
    // (not the story cascade) driving the option removal.
    sqlx::query("DELETE FROM open_questions WHERE id = ?")
        .bind("oq1")
        .execute(&pool)
        .await
        .expect("delete open_question");
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM open_questions").await,
        0,
        "open_question should be gone"
    );
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM question_options").await,
        0,
        "question_options should cascade-delete with their open_question"
    );

    // (a) Deleting the story cascade-deletes its acceptance_criteria and
    // research_notes. (Re-insert an open_question to also prove that collection
    // cascades from the work_item delete path.)
    sqlx::query(
        "INSERT INTO open_questions (id, story_id, seq, question, status) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind("oq2")
    .bind(&story)
    .bind(2_i64)
    .bind("another question")
    .bind("open")
    .execute(&pool)
    .await
    .expect("re-insert open_question for story-cascade check");
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM open_questions").await, 1);

    sqlx::query("DELETE FROM work_items WHERE id = ?")
        .bind(&story)
        .execute(&pool)
        .await
        .expect("delete story work_item");

    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM acceptance_criteria").await,
        0,
        "acceptance_criteria should cascade-delete with their work_item"
    );
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM research_notes").await,
        0,
        "research_notes should cascade-delete with their work_item"
    );
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM open_questions").await,
        0,
        "open_questions should cascade-delete with their story work_item"
    );
}
