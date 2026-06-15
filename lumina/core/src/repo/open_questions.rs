//! Open questions + options + branch resolution (migration 0003, R4 carve). A
//! story-scoped decision lifecycle: add a question + N options, block tasks on
//! the question, tie a task to a branch (`enabling_option_id`), then resolve —
//! picking an option unblocks the chosen branch and cancels the other branches'
//! exclusive tasks.
//!
//! The shared substrate these compose on (`work_item_kind`) lives in
//! `repo/shared.rs` and is reached via `use super::*`; the event-outbox writer
//! comes from `super::events`.
//!
//! `pub use open_questions::*` in `repo/mod.rs` PRESERVES the public surface —
//! every `pub` fn here stays reachable at its existing `crate::repo::*` path (the
//! HTTP handlers / MCP tools / importer call them by path and are unchanged). The
//! domain types named in the signatures are imported explicitly from `crate::*`
//! (a `use super::*` glob does NOT carry super's private `use` imports).

use uuid::Uuid;

use super::*;
use super::events::record_event;
use crate::args;
use crate::db::{DbClient, Scalar};
use crate::error::AppError;

/// Append ONE `open_questions` row under the single-mutation-path discipline
/// (migration 0003). Story-scoped: a non-`story` target is rejected with a typed
/// [`AppError::Validation`] (kind read first; this also yields `NotFound` if the
/// id is absent). `seq` = `MAX(seq)+1` per story; `status` defaults to `open`.
/// Event `open_question.added`. Returns the new question id.
pub async fn add_open_question(
    db: &impl DbClient,
    story_id: &str,
    question: &str,
) -> Result<Uuid, AppError> {
    let kind = work_item_kind(db, story_id).await?;
    if kind != "story" {
        return Err(AppError::Validation(format!(
            "open questions are settable only on a story, not on '{kind}'"
        )));
    }

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM open_questions WHERE story_id = $1",
        args![story_id.to_owned()],
    )
    .await?;

    tx.execute(
        "INSERT INTO open_questions (id, story_id, seq, question, status) VALUES ($1, $2, $3, $4, 'open')",
        args![id_str.clone(), story_id.to_owned(), seq, question.to_owned()],
    )
    .await?;

    // Route the event to the owning STORY's work_item aggregate (R1): export only
    // renders work_item aggregates, so an `open_question`-typed event would never
    // reach the git-export snapshot. event_type/payload are otherwise unchanged,
    // so the "exactly one event" invariant holds.
    let payload = serde_json::json!({ "question_id": id_str, "seq": seq });
    record_event(tx.as_mut(), "work_item", story_id, "open_question.added", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Read an open question's owning `story_id`, erroring `NotFound` if the question
/// id has no row. Used by the option-add and resolve paths.
async fn open_question_story(db: &impl DbClient, id: &str) -> Result<String, AppError> {
    crate::db::scalar_opt::<String>(
        db,
        "SELECT story_id FROM open_questions WHERE id = $1",
        args![id.to_owned()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("open_question '{id}' not found")))
}

/// Append ONE `question_options` row under the single-mutation-path discipline
/// (migration 0003). `seq` = `MAX(seq)+1` per question; the question must exist
/// (`NotFound` otherwise). Event `open_question.option_added`. Returns the new
/// option id.
pub async fn add_question_option(
    db: &impl DbClient,
    question_id: &str,
    label: &str,
    detail: Option<&str>,
) -> Result<Uuid, AppError> {
    // Verify the question exists first (NotFound, not a dangling-FK 500) AND
    // capture its owning story for the event aggregate (R1).
    let story_id = open_question_story(db, question_id).await?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM question_options WHERE question_id = $1",
        args![question_id.to_owned()],
    )
    .await?;

    tx.execute(
        "INSERT INTO question_options (id, question_id, seq, label, detail) VALUES ($1, $2, $3, $4, $5)",
        args![
            id_str.clone(),
            question_id.to_owned(),
            seq,
            label.to_owned(),
            detail.map(str::to_owned),
        ],
    )
    .await?;

    // Route to the owning STORY's work_item aggregate (R1) so export renders it.
    let payload = serde_json::json!({ "option_id": id_str, "seq": seq });
    record_event(tx.as_mut(), "work_item", &story_id, "open_question.option_added", payload)
        .await?;

    tx.commit().await?;
    Ok(id)
}

/// Block a task on an open question (migration 0003): set
/// `blocked_by_question_id = question_id` AND `status = 'blocked'` in one write.
/// One event `work_item.blocked_on_question`; `NotFound` via `rows_affected()==0`.
///
/// Task-scoped (R3): a non-`task` kind is rejected with a typed
/// [`AppError::Validation`] (mirrors [`set_effort`]), and the referenced
/// `question_id` must exist (else `Validation`, not a dangling-FK 500). The
/// task's current status must be `todo`/`open` (R12): the branch-resolution
/// model restores blocked tasks to `todo` on unblock, so blocking an
/// `in_progress`/`done` task would silently lose its state — that is rejected
/// with `Validation` rather than clobbered.
pub async fn block_task_on_question(
    db: &impl DbClient,
    task_id: &str,
    question_id: &str,
) -> Result<(), AppError> {
    // Task-scoped guard (R3); also yields NotFound if the id is absent.
    let kind = work_item_kind(db, task_id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "block_task_on_question is settable only on a task, not on '{kind}'"
        )));
    }

    // The referenced question must exist (R3): clean 422 over a dangling-FK 500.
    let q_exists = db
        .query_opt::<Scalar<i64>>(
            "SELECT 1 FROM open_questions WHERE id = $1",
            args![question_id.to_owned()],
        )
        .await?
        .is_some();
    if !q_exists {
        return Err(AppError::Validation(format!(
            "open_question '{question_id}' does not exist"
        )));
    }

    // R12: only block a pre-todo task. Blocking an in_progress/done task would be
    // silently downgraded to `todo` on unblock, losing state — reject instead.
    let current = crate::db::scalar_one::<String>(
        db,
        "SELECT status FROM work_items WHERE id = $1",
        args![task_id.to_owned()],
    )
    .await?;
    if !matches!(current.as_str(), "todo" | "open") {
        return Err(AppError::Validation(format!(
            "task '{task_id}' cannot be blocked from status '{current}': only a 'todo'/'open' \
             task may be blocked (the branch-resolution model restores blocked tasks to 'todo')"
        )));
    }

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE work_items
        SET blocked_by_question_id = $2, status = 'blocked', updated_at = CURRENT_TIMESTAMP
        WHERE id = $1
        "#,
            args![task_id.to_owned(), question_id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{task_id}' not found")));
    }

    let payload = serde_json::json!({ "blocked_by_question_id": question_id });
    record_event(tx.as_mut(), "work_item", task_id, "work_item.blocked_on_question", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Tie a task to a specific answer-option branch (migration 0003): set
/// `enabling_option_id = option_id` (the exclusive-branch marker — a task with
/// this set is cancelled if a DIFFERENT option is chosen on resolution). One
/// event `work_item.enabling_option_set`; `NotFound` via `rows_affected()==0`.
///
/// Task-scoped (R3): a non-`task` kind is rejected with a typed
/// [`AppError::Validation`] (mirrors [`set_effort`]), and the referenced
/// `option_id` must exist (else `Validation`, not a dangling-FK 500).
pub async fn set_enabling_option(
    db: &impl DbClient,
    task_id: &str,
    option_id: &str,
) -> Result<(), AppError> {
    // Task-scoped guard (R3); also yields NotFound if the id is absent.
    let kind = work_item_kind(db, task_id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "set_enabling_option is settable only on a task, not on '{kind}'"
        )));
    }

    // The referenced option must exist (R3): clean 422 over a dangling-FK 500.
    let opt_exists = db
        .query_opt::<Scalar<i64>>(
            "SELECT 1 FROM question_options WHERE id = $1",
            args![option_id.to_owned()],
        )
        .await?
        .is_some();
    if !opt_exists {
        return Err(AppError::Validation(format!(
            "question_option '{option_id}' does not exist"
        )));
    }

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE work_items
        SET enabling_option_id = $2, updated_at = CURRENT_TIMESTAMP
        WHERE id = $1
        "#,
            args![task_id.to_owned(), option_id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{task_id}' not found")));
    }

    let payload = serde_json::json!({ "enabling_option_id": option_id });
    record_event(tx.as_mut(), "work_item", task_id, "work_item.enabling_option_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Resolve an open question by picking an answer option (migration 0003).
///
/// This is the one multi-write mutation in the module: in ONE transaction it
///   1. marks the question `status='answered'`, stamps `chosen_option_id`,
///      `decided_at`, `decided_by`;
///   2. transitions the CHOSEN branch's blocked tasks `blocked → todo` — both the
///      exclusive tasks tied to the chosen option AND any non-exclusive blocked
///      task (NULL `enabling_option_id`) on this question;
///   3. transitions the OTHER branches' EXCLUSIVE blocked tasks (a non-NULL
///      `enabling_option_id` that is NOT the chosen one) to `status='cancelled'`.
///
/// It emits EXACTLY ONE `open_question.resolved` event for the whole resolution
/// (NOT one per task), preserving the +1-event-per-logical-write invariant.
///
/// `chosen_option_id` must belong to the question (else `Validation`). `NotFound`
/// if the question is absent (checked before any write).
pub async fn resolve_open_question(
    db: &impl DbClient,
    question_id: &str,
    chosen_option_id: &str,
    by: Option<&str>,
) -> Result<(), AppError> {
    // NotFound if the question is absent (before any write); capture the owning
    // story for the event aggregate (R1).
    let story_id = open_question_story(db, question_id).await?;

    // Reject re-resolving an already-answered/cancelled question (R4) so the
    // advertised idempotency is real rather than silently re-running the branch
    // transitions on a second call. `status` is a nullable column, so it reads
    // back as `Option<String>` (NULL → None).
    let status = crate::db::scalar_one::<Option<String>>(
        db,
        "SELECT status FROM open_questions WHERE id = $1",
        args![question_id.to_owned()],
    )
    .await?;
    if status.as_deref() != Some("open") {
        return Err(AppError::Validation(format!(
            "open_question '{question_id}' already resolved/cancelled (status {})",
            status.as_deref().unwrap_or("unknown")
        )));
    }

    // Validate the chosen option belongs to THIS question.
    let owns = crate::db::scalar_one::<i64>(
        db,
        "SELECT COUNT(*) FROM question_options WHERE id = $1 AND question_id = $2",
        args![chosen_option_id.to_owned(), question_id.to_owned()],
    )
    .await?;
    if owns == 0 {
        return Err(AppError::Validation(format!(
            "option '{chosen_option_id}' does not belong to open_question '{question_id}'"
        )));
    }

    let mut tx = db.begin().await?;

    // 1. Mark the question answered.
    tx.execute(
        r#"
        UPDATE open_questions
        SET status = 'answered',
            chosen_option_id = $2,
            decided_at = CURRENT_TIMESTAMP,
            decided_by = $3
        WHERE id = $1
        "#,
        args![question_id.to_owned(), chosen_option_id.to_owned(), by.map(str::to_owned)],
    )
    .await?;

    // 2. Unblock the chosen branch: blocked tasks on this question whose
    //    enabling_option is the chosen one OR is NULL (non-exclusive) → todo.
    tx.execute(
        r#"
        UPDATE work_items
        SET status = 'todo', updated_at = CURRENT_TIMESTAMP
        WHERE blocked_by_question_id = $1
          AND status = 'blocked'
          AND (enabling_option_id = $2 OR enabling_option_id IS NULL)
        "#,
        args![question_id.to_owned(), chosen_option_id.to_owned()],
    )
    .await?;

    // 3. Cancel the other branches' EXCLUSIVE tasks: blocked tasks on this
    //    question with a non-NULL enabling_option that is NOT the chosen one.
    tx.execute(
        r#"
        UPDATE work_items
        SET status = 'cancelled', updated_at = CURRENT_TIMESTAMP
        WHERE blocked_by_question_id = $1
          AND status = 'blocked'
          AND enabling_option_id IS NOT NULL
          AND enabling_option_id <> $2
        "#,
        args![question_id.to_owned(), chosen_option_id.to_owned()],
    )
    .await?;

    // EXACTLY ONE event for the whole resolution (NOT per task). Routed to the
    // owning STORY's work_item aggregate (R1) so export renders it; `question_id`
    // is carried so the export drain can re-render this question's affected tasks
    // (R2) without a per-task event.
    let payload =
        serde_json::json!({ "chosen_option_id": chosen_option_id, "question_id": question_id });
    record_event(tx.as_mut(), "work_item", &story_id, "open_question.resolved", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Durable ESCALATION primitive (focus 1C.1): raise a story-scoped hard
/// decision AND park the deciding task on it, ATOMICALLY in one write
/// transaction. This is the agent→human handoff's ESCALATE half; its symmetric
/// counterpart is [`resolve_open_question`] (the human→agent return half, which
/// already unblocks the chosen branch + cancels the others).
///
/// Composes the SAME SQL the three single-mutation primitives in this module
/// run, inlined into one transaction so a partial failure can never leave a
/// dangling question with an un-parked task (or vice versa):
///   1. INSERT the `open_questions` row (mirrors [`add_open_question`]'s body),
///   2. INSERT one `question_options` row per label (mirrors
///      [`add_question_option`]'s body),
///   3. UPDATE the task to `status='blocked'` + `blocked_by_question_id`
///      (mirrors [`block_task_on_question`]'s body).
///
/// Why park via the BLOCK mechanism and NEVER a held lease (research note
/// seq17): an autonomous agent that escalated under a held team-execution lease
/// would have that lease lazily reclaimed to `todo` once it expired, and a fresh
/// agent would re-ask the very question already pending a human answer. The
/// claim's readiness predicate excludes `status='blocked'` tasks, so blocking is
/// the only park that survives the asking agent going away. (R12, mirrored from
/// `block_task_on_question`: only a `todo`/`open` task may be parked — blocking
/// an `in_progress`/`done` task would be silently downgraded to `todo` on
/// unblock, losing state — so this rejects a non-pre-`todo` task with
/// `Validation`.)
///
/// Why one transaction is the HARD-STOP contract (research note seq18): a failed
/// durable-channel write must propagate the [`AppError`] so the caller stops
/// rather than degrades — doing the raise + park in one tx means any failure
/// rolls BOTH back, so the agent never proceeds past an un-recorded decision and
/// never leaves an orphan question for a human to puzzle over.
///
/// Validation order mirrors the composed primitives and runs BEFORE any write:
/// the story must be a `story` kind, the task must be a `task` kind, and the
/// task must be `todo`/`open`. Emits EXACTLY ONE `open_question.escalated` event
/// on the owning STORY's `work_item` aggregate (R1: export renders only
/// `work_item` aggregates), preserving the +1-event-per-logical-write invariant
/// the way [`resolve_open_question`] does for its multi-write resolution.
///
/// Returns the new question id.
pub async fn escalate_decision_and_park_task(
    db: &impl DbClient,
    story_id: &str,
    task_id: &str,
    question: &str,
    options: &[&str],
) -> Result<Uuid, AppError> {
    // ---- Validate (auto-commit reads, before any write) ----------------
    // Story-scoped question (mirrors `add_open_question`); also NotFound if absent.
    let story_kind = work_item_kind(db, story_id).await?;
    if story_kind != "story" {
        return Err(AppError::Validation(format!(
            "open questions are settable only on a story, not on '{story_kind}'"
        )));
    }

    // Task-scoped park (mirrors `block_task_on_question`); also NotFound if absent.
    let task_kind = work_item_kind(db, task_id).await?;
    if task_kind != "task" {
        return Err(AppError::Validation(format!(
            "escalate_decision_and_park_task parks only a task, not a '{task_kind}'"
        )));
    }

    // R12 (mirrored from `block_task_on_question`): only a pre-`todo` task may be
    // parked — blocking an in_progress/done task would be silently downgraded to
    // `todo` on unblock, losing state.
    let current = crate::db::scalar_one::<String>(
        db,
        "SELECT status FROM work_items WHERE id = $1",
        args![task_id.to_owned()],
    )
    .await?;
    if !matches!(current.as_str(), "todo" | "open") {
        return Err(AppError::Validation(format!(
            "task '{task_id}' cannot be parked from status '{current}': only a 'todo'/'open' \
             task may be blocked (the branch-resolution model restores blocked tasks to 'todo')"
        )));
    }

    let question_id = Uuid::now_v7();
    let question_id_str = question_id.to_string();

    // ---- One write transaction: raise + options + park (seq18 hard-stop) --
    let mut tx = db.begin().await?;

    // 1. INSERT the question (mirrors `add_open_question`).
    let q_seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM open_questions WHERE story_id = $1",
        args![story_id.to_owned()],
    )
    .await?;
    tx.execute(
        "INSERT INTO open_questions (id, story_id, seq, question, status) VALUES ($1, $2, $3, $4, 'open')",
        args![question_id_str.clone(), story_id.to_owned(), q_seq, question.to_owned()],
    )
    .await?;

    // 2. INSERT each option (mirrors `add_question_option`); `seq` is 1-based and
    //    monotonic within this question, so an empty `options` slice simply adds
    //    none (a free-text-only escalation).
    for (idx, label) in options.iter().enumerate() {
        let opt_id = Uuid::now_v7().to_string();
        let opt_seq = idx as i64 + 1;
        tx.execute(
            "INSERT INTO question_options (id, question_id, seq, label, detail) VALUES ($1, $2, $3, $4, NULL)",
            args![opt_id, question_id_str.clone(), opt_seq, (*label).to_owned()],
        )
        .await?;
    }

    // 3. Park the task via the BLOCK mechanism (seq17 — never a held lease).
    //    Mirrors `block_task_on_question`'s UPDATE; the kind/status guards above
    //    already ran, so a `rows_affected()==0` here is a concurrent-delete race.
    let affected = tx
        .execute(
            r#"
        UPDATE work_items
        SET blocked_by_question_id = $2, status = 'blocked', updated_at = CURRENT_TIMESTAMP
        WHERE id = $1
        "#,
            args![task_id.to_owned(), question_id_str.clone()],
        )
        .await?;
    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{task_id}' not found")));
    }

    // EXACTLY ONE event for the whole escalate logical write, routed to the
    // owning STORY's work_item aggregate (R1) so export renders it — symmetric
    // with `resolve_open_question`'s single `open_question.resolved` event.
    let payload = serde_json::json!({
        "question_id": question_id_str,
        "parked_task_id": task_id,
    });
    record_event(tx.as_mut(), "work_item", story_id, "open_question.escalated", payload).await?;

    tx.commit().await?;
    Ok(question_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::repo::test_support::*;

    /// `add_open_question` on a non-story (here: a task) returns a typed
    /// `Validation`, and succeeds on a story.
    #[tokio::test]
    async fn add_open_question_rejects_non_story() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();

        let err = add_open_question(&pool, &task, "should we?")
            .await
            .expect_err("open question on a task must reject");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        add_open_question(&pool, &story, "should we?")
            .await
            .expect("open question on a story ok");
    }

    /// Resolving a two-option question unblocks the chosen branch's task (→todo)
    /// and cancels the other branch's exclusive task (→cancelled); a non-exclusive
    /// blocked task on the question is also unblocked; and the whole multi-write
    /// resolution emits EXACTLY ONE `open_question.resolved` event.
    #[tokio::test]
    async fn resolve_open_question_branches_and_one_event() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let q = add_open_question(&pool, &story, "which approach?")
            .await
            .expect("question")
            .to_string();
        let opt_a = add_question_option(&pool, &q, "A", None).await.expect("opt A").to_string();
        let opt_b = add_question_option(&pool, &q, "B", None).await.expect("opt B").to_string();

        // Three branch tasks: exclusive-to-A, exclusive-to-B, and non-exclusive.
        let task_a = create_work_item(&pool, "task", Some(&story), "TA", None)
            .await
            .expect("task A")
            .to_string();
        let task_b = create_work_item(&pool, "task", Some(&story), "TB", None)
            .await
            .expect("task B")
            .to_string();
        let task_shared = create_work_item(&pool, "task", Some(&story), "TS", None)
            .await
            .expect("task shared")
            .to_string();

        block_task_on_question(&pool, &task_a, &q).await.expect("block A");
        set_enabling_option(&pool, &task_a, &opt_a).await.expect("tie A");
        block_task_on_question(&pool, &task_b, &q).await.expect("block B");
        set_enabling_option(&pool, &task_b, &opt_b).await.expect("tie B");
        block_task_on_question(&pool, &task_shared, &q).await.expect("block shared");

        assert_eq!(item_status(&pool, &task_a).await, "blocked");
        assert_eq!(item_status(&pool, &task_b).await, "blocked");
        assert_eq!(item_status(&pool, &task_shared).await, "blocked");

        let resolved_before = count_events_of_type(&pool, "open_question.resolved").await;
        let ev_before = count_events(&pool).await;

        // Choose option A.
        resolve_open_question(&pool, &q, &opt_a, Some("alice"))
            .await
            .expect("resolve");

        // Chosen branch (A) and non-exclusive (shared) → todo; other branch (B)
        // → cancelled.
        assert_eq!(item_status(&pool, &task_a).await, "todo", "chosen-branch task unblocked");
        assert_eq!(item_status(&pool, &task_shared).await, "todo", "non-exclusive task unblocked");
        assert_eq!(item_status(&pool, &task_b).await, "cancelled", "other-branch task cancelled");

        // Question is answered with the chosen option recorded.
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        let folded = detail
            .open_questions
            .iter()
            .find(|oq| oq.id == q)
            .expect("question folded into detail");
        assert_eq!(folded.status.as_deref(), Some("answered"));
        assert_eq!(folded.chosen_option_id.as_deref(), Some(opt_a.as_str()));
        assert_eq!(folded.options.len(), 2, "both options folded");

        // EXACTLY ONE resolved event for the whole multi-write resolution.
        assert_eq!(
            count_events_of_type(&pool, "open_question.resolved").await,
            resolved_before + 1,
            "exactly one open_question.resolved event for the resolution"
        );
        assert_eq!(
            count_events(&pool).await,
            ev_before + 1,
            "the multi-write resolution adds exactly one events row"
        );

        // Resolving with an option from a DIFFERENT question is Validation.
        let q2 = add_open_question(&pool, &story, "another?").await.expect("q2").to_string();
        let err = resolve_open_question(&pool, &q2, &opt_a, None)
            .await
            .expect_err("foreign option must reject");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // Resolving a missing question is NotFound.
        let err = resolve_open_question(&pool, "missing", &opt_a, None)
            .await
            .expect_err("missing question");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
    }

    /// The durable escalation primitive (focus 1C.1) atomically raises a
    /// story-scoped question, adds its options, and PARKS the task via the BLOCK
    /// mechanism (`status='blocked'` + `blocked_by_question_id`, NOT a lease —
    /// seq17). The whole raise+park is ONE event. Then its symmetric counterpart
    /// `resolve_open_question` unblocks the parked task back to `todo`, proving
    /// the agent→human→agent round-trip closes.
    #[tokio::test]
    async fn escalate_decision_parks_via_block_and_resolve_unblocks() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();

        let ev_before = count_events(&pool).await;

        let q = escalate_decision_and_park_task(
            &pool,
            &story,
            &task,
            "ship which way?",
            &["A", "B"],
        )
        .await
        .expect("escalate")
        .to_string();

        // The question exists, is folded into the story detail with both options.
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        let folded = detail
            .open_questions
            .iter()
            .find(|oq| oq.id == q)
            .expect("escalated question folded into story detail");
        assert_eq!(folded.status.as_deref(), Some("open"));
        assert_eq!(folded.options.len(), 2, "both options inserted");

        // The task is PARKED via the block mechanism: blocked + back-linked.
        assert_eq!(item_status(&pool, &task).await, "blocked", "task parked via block");
        let blocked_by = sqlx::query_scalar::<_, Option<String>>(
            "SELECT blocked_by_question_id FROM work_items WHERE id = ?1",
        )
        .bind(&task)
        .fetch_one(&pool)
        .await
        .expect("blocked_by read");
        assert_eq!(
            blocked_by.as_deref(),
            Some(q.as_str()),
            "blocked_by_question_id back-links the escalated question"
        );

        // EXACTLY ONE event for the whole atomic raise+park.
        assert_eq!(
            count_events(&pool).await,
            ev_before + 1,
            "the atomic escalate adds exactly one events row"
        );
        assert_eq!(
            count_events_of_type(&pool, "open_question.escalated").await,
            1,
            "exactly one open_question.escalated event"
        );

        // Sanity: the symmetric return half unblocks the parked task → todo.
        let chosen = folded.options[0].id.clone();
        resolve_open_question(&pool, &q, &chosen, Some("human"))
            .await
            .expect("resolve");
        assert_eq!(
            item_status(&pool, &task).await,
            "todo",
            "resolve unblocks the parked task back to todo"
        );
    }

    /// Escalation refuses to park a non-pre-`todo` task (R12, mirrored from
    /// `block_task_on_question`): an `in_progress` task is rejected with
    /// `Validation` and NO question is written (the atomic guard runs before any
    /// write).
    #[tokio::test]
    async fn escalate_rejects_non_todo_task_and_writes_nothing() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        sqlx::query("UPDATE work_items SET status = 'in_progress' WHERE id = ?1")
            .bind(&task)
            .execute(&pool)
            .await
            .expect("force in_progress");

        let q_before = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM open_questions")
            .fetch_one(&pool)
            .await
            .expect("count");

        let err = escalate_decision_and_park_task(&pool, &story, &task, "?", &["A"])
            .await
            .expect_err("non-todo task must reject");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        let q_after = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM open_questions")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(q_before, q_after, "no question written when park is rejected");
    }
}
