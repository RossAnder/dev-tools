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

/// Idempotent RUN-LEVEL question creation (focus 1C.1, research notes seq22 +
/// seq29). `open_questions` is STORY-scoped, so a run-level decision is REUSED as
/// a story-scoped row TAGGED to the run via a caller-supplied dedup/idempotency
/// `dedup_key` (the "run tag" lives in the key — typically `<run_id>:<slug>` or a
/// content hash). The gap this closes (seq29): two non-PTY teammates hitting the
/// SAME run-level decision would each call [`add_open_question`] and create
/// DUPLICATE questions. This path COLLAPSES the second create onto the first.
///
/// Contract: within ONE write transaction (`BEGIN IMMEDIATE` serialises writers
/// at begin-time, so two concurrent in-process callers can't both pass the
/// pre-check), if an OPEN question on `story_id` already carries `dedup_key`,
/// return its id WITHOUT writing — `(false, existing_id)`; else INSERT a fresh
/// row stamping `run_dedup_key = dedup_key` and return `(true, new_id)`. The
/// partial UNIQUE index `idx_open_questions_run_dedup` (migration 0022,
/// `(story_id, run_dedup_key) WHERE run_dedup_key IS NOT NULL AND status='open'`)
/// is the record-layer backstop against a race the pre-check misses. The key is
/// scoped to OPEN rows, so once a run-level question is answered/cancelled the
/// same key may be re-raised for a fresh decision.
///
/// Story-scoped (mirrors [`add_open_question`]): a non-`story` target is rejected
/// with [`AppError::Validation`] (kind read first; also yields `NotFound` if the
/// id is absent). A create emits ONE `open_question.added` event on the owning
/// story's `work_item` aggregate (R1); a collapse-onto-existing emits NO event
/// (no logical write happened), preserving the +1-event-per-write invariant.
///
/// Returns `(created, question_id)` — `created` is `false` on a dedup collapse.
pub async fn add_run_question_idempotent(
    db: &impl DbClient,
    story_id: &str,
    question: &str,
    dedup_key: &str,
) -> Result<(bool, Uuid), AppError> {
    let kind = work_item_kind(db, story_id).await?;
    if kind != "story" {
        return Err(AppError::Validation(format!(
            "open questions are settable only on a story, not on '{kind}'"
        )));
    }

    let mut tx = db.begin().await?;

    // Pre-check INSIDE the write tx: an existing OPEN question on this story
    // carrying the key collapses the create. BEGIN IMMEDIATE holds the RESERVED
    // lock from begin-time, so a concurrent in-process caller serialises behind
    // this read→insert rather than racing it; the partial UNIQUE index backstops
    // any record-layer race the pre-check cannot see.
    let existing = crate::db::tx_scalar_opt::<String>(
        tx.as_mut(),
        "SELECT id FROM open_questions WHERE story_id = $1 AND run_dedup_key = $2 AND status = 'open'",
        args![story_id.to_owned(), dedup_key.to_owned()],
    )
    .await?;
    if let Some(id_str) = existing {
        // Collapse onto the existing run-level question: no write, no event.
        let id = Uuid::parse_str(&id_str)
            .map_err(|e| AppError::Validation(format!("stored question id '{id_str}' is not a uuid: {e}")))?;
        tx.commit().await?;
        return Ok((false, id));
    }

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM open_questions WHERE story_id = $1",
        args![story_id.to_owned()],
    )
    .await?;

    tx.execute(
        "INSERT INTO open_questions (id, story_id, seq, question, status, run_dedup_key) VALUES ($1, $2, $3, $4, 'open', $5)",
        args![id_str.clone(), story_id.to_owned(), seq, question.to_owned(), dedup_key.to_owned()],
    )
    .await?;

    // Route the event to the owning STORY's work_item aggregate (R1) — same shape
    // as `add_open_question`, so export + the "exactly one event" invariant hold.
    let payload = serde_json::json!({ "question_id": id_str, "seq": seq });
    record_event(tx.as_mut(), "work_item", story_id, "open_question.added", payload).await?;

    tx.commit().await?;
    Ok((true, id))
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

    // Resume epoch-guard + resolved-after-close (focus 1C.1, research note seq19 +
    // edge note seq28). The escalate path stamps `resume_epoch` = the parked
    // task's lifetime event count at ask-time; questions raised via plain
    // `add_open_question` leave it NULL (unguarded — they have no single parked
    // consumer to version against, so they resolve exactly as before).
    let resume_epoch = crate::db::scalar_one::<Option<i64>>(
        db,
        "SELECT resume_epoch FROM open_questions WHERE id = $1",
        args![question_id.to_owned()],
    )
    .await?;

    // The parked consumer(s) still waiting on this question. The escalate path
    // parks EXACTLY ONE task (back-linked via `blocked_by_question_id`); a
    // plain question may have 0+ manually-blocked tasks. If NONE remain blocked,
    // the run/session that asked has clean-closed (the task was cancelled, done,
    // re-planned, or never parked) and the human's answer has no live consumer to
    // re-initiate — that is the seq28 edge, recorded by the `resolved_after_close`
    // marker so the deferred 1C.3 scheduler can sweep + re-initiate it later
    // rather than strand it.
    let parked_task = crate::db::scalar_opt::<String>(
        db,
        "SELECT id FROM work_items \
         WHERE blocked_by_question_id = $1 AND status = 'blocked' AND deleted_at IS NULL \
         ORDER BY id LIMIT 1",
        args![question_id.to_owned()],
    )
    .await?;
    let resolved_after_close = parked_task.is_none();

    // Stale-resolution guard: only when an epoch was stamped (escalate path) AND
    // a parked task still waits. Re-read the parked task's CURRENT lifetime event
    // count; if it differs from the ask-time epoch the task's state shifted
    // between ask and answer (e.g. the story was re-planned), so the resolution is
    // STALE — reject it (detectable to the caller) rather than silently apply it.
    if let (Some(epoch), Some(task)) = (resume_epoch, parked_task.as_deref()) {
        let now_epoch = crate::db::scalar_one::<i64>(
            db,
            "SELECT COUNT(*) FROM events WHERE aggregate_id = $1 AND aggregate_type = 'work_item'",
            args![task.to_owned()],
        )
        .await?;
        if now_epoch != epoch {
            return Err(AppError::Validation(format!(
                "open_question '{question_id}' resolution is STALE: the parked task's state \
                 changed since the question was asked (resume_epoch {epoch} != current {now_epoch}); \
                 re-ask against the current state before resolving"
            )));
        }
    }

    let mut tx = db.begin().await?;

    // 1. Mark the question answered. When no parked consumer remained, stamp the
    //    `resolved_after_close` marker (CURRENT_TIMESTAMP) so the deferred 1C.3
    //    scheduler can later sweep late resolutions; otherwise leave it NULL.
    //    A bound CASE keeps this one statement: the marker is set iff
    //    `resolved_after_close` is true (no live parked task).
    tx.execute(
        r#"
        UPDATE open_questions
        SET status = 'answered',
            chosen_option_id = $2,
            decided_at = CURRENT_TIMESTAMP,
            decided_by = $3,
            resolved_after_close = CASE WHEN $4 THEN CURRENT_TIMESTAMP ELSE NULL END
        WHERE id = $1
        "#,
        args![
            question_id.to_owned(),
            chosen_option_id.to_owned(),
            by.map(str::to_owned),
            resolved_after_close,
        ],
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

    // Resume epoch-guard (focus 1C.1, research note seq19): capture the parked
    // task's lifetime event count NOW, at ask-time, and stamp it on the question
    // as `resume_epoch`. Every repo mutation on a work_item records exactly one
    // `events` row on that item's aggregate, so this count is a monotonic,
    // deterministic state-version of the task. `resolve_open_question` later
    // re-reads the count and compares: if it grew (the task was re-planned /
    // its state shifted between ask and answer), the human's resolution is STALE
    // and is rejected rather than silently applied to a changed task. Read on the
    // auto-commit connection BEFORE the write tx — it is a point-in-time snapshot
    // either way, and the block UPDATE below records no per-task event (it routes
    // its single event to the story), so this count is the task's pre-park value.
    let resume_epoch = crate::db::scalar_one::<i64>(
        db,
        "SELECT COUNT(*) FROM events WHERE aggregate_id = $1 AND aggregate_type = 'work_item'",
        args![task_id.to_owned()],
    )
    .await?;

    let question_id = Uuid::now_v7();
    let question_id_str = question_id.to_string();

    // ---- One write transaction: raise + options + park (seq18 hard-stop) --
    let mut tx = db.begin().await?;

    // 1. INSERT the question (mirrors `add_open_question`), stamping the
    //    ask-time `resume_epoch` so the resolve half can detect a stale answer.
    let q_seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM open_questions WHERE story_id = $1",
        args![story_id.to_owned()],
    )
    .await?;
    tx.execute(
        "INSERT INTO open_questions (id, story_id, seq, question, status, resume_epoch) VALUES ($1, $2, $3, $4, 'open', $5)",
        args![question_id_str.clone(), story_id.to_owned(), q_seq, question.to_owned(), resume_epoch],
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

    /// Run-level idempotent creation (focus 1C.1, seq29): a SECOND create with
    /// the SAME dedup key on the SAME story does NOT duplicate — the count stays
    /// 1 and the returned id matches the first, with `created=false` and NO extra
    /// `open_question.added` event. A DIFFERENT key on the same story is a fresh
    /// question (count rises), and once the first is resolved its key frees so a
    /// re-raise creates anew.
    #[tokio::test]
    async fn run_question_idempotent_collapses_on_same_key() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let added_before = count_events_of_type(&pool, "open_question.added").await;

        // First create: a real INSERT.
        let (created1, q1) =
            add_run_question_idempotent(&pool, &story, "run-level: which target branch?", "run-7:branch")
                .await
                .expect("first create");
        assert!(created1, "first create writes a new question");

        // Second create with the SAME key: collapses onto the first, no write.
        let (created2, q2) =
            add_run_question_idempotent(&pool, &story, "run-level: which target branch?", "run-7:branch")
                .await
                .expect("second create");
        assert!(!created2, "second create with the same key does NOT write");
        assert_eq!(q1, q2, "the collapse returns the SAME question id");

        // Exactly ONE open_questions row exists for this key, and exactly ONE
        // open_question.added event fired across the two calls.
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM open_questions WHERE story_id = ?1 AND run_dedup_key = ?2",
        )
        .bind(&story)
        .bind("run-7:branch")
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(count, 1, "the dedup key never duplicates a live run-level question");
        assert_eq!(
            count_events_of_type(&pool, "open_question.added").await,
            added_before + 1,
            "exactly one open_question.added event for the two idempotent creates"
        );

        // A DIFFERENT key on the same story is a distinct question.
        let (created3, q3) =
            add_run_question_idempotent(&pool, &story, "run-level: ship now?", "run-7:ship")
                .await
                .expect("different key");
        assert!(created3 && q3 != q1, "a different key creates a fresh question");

        // Once the first is resolved, its key frees: a re-raise creates anew.
        let opt = add_question_option(&pool, &q1.to_string(), "A", None).await.expect("opt").to_string();
        resolve_open_question(&pool, &q1.to_string(), &opt, Some("human")).await.expect("resolve");
        let (created4, q4) =
            add_run_question_idempotent(&pool, &story, "run-level: which target branch?", "run-7:branch")
                .await
                .expect("re-raise after resolve");
        assert!(created4 && q4 != q1, "a resolved key frees for a fresh run-level question");

        // Non-story target rejects (mirrors add_open_question).
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        let err = add_run_question_idempotent(&pool, &task, "?", "k")
            .await
            .expect_err("non-story must reject");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
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

    /// Resume epoch-guard + resolved-after-close marker (focus 1C.1, migration
    /// 0021). The escalate path stamps `resume_epoch` = the parked task's
    /// ask-time event count. THREE outcomes are exercised:
    ///   (a) the parked task's state changes after the ask (here: a `set_effort`
    ///       records a new task event) → the resolution is STALE and rejected
    ///       with `Validation`, with NO question state mutated;
    ///   (b) a clean (epoch-matching, still-parked) answer resolves normally and
    ///       leaves `resolved_after_close` NULL;
    ///   (c) when no parked consumer remains (the task was cancelled away before
    ///       the answer arrived), the answer still resolves but stamps the
    ///       durable `resolved_after_close` marker for the deferred scheduler.
    #[tokio::test]
    async fn resume_epoch_detects_stale_resolution_and_after_close_marker() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        // --- (a) STALE: the parked task changes between ask and answer. -------
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        let q = escalate_decision_and_park_task(&pool, &story, &task, "which?", &["A", "B"])
            .await
            .expect("escalate")
            .to_string();

        // The epoch was stamped at ask-time (non-NULL on the escalate path).
        let epoch = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT resume_epoch FROM open_questions WHERE id = ?1",
        )
        .bind(&q)
        .fetch_one(&pool)
        .await
        .expect("epoch read");
        assert!(epoch.is_some(), "escalate stamps a resume_epoch");

        let opt = sqlx::query_scalar::<_, String>(
            "SELECT id FROM question_options WHERE question_id = ?1 ORDER BY seq LIMIT 1",
        )
        .bind(&q)
        .fetch_one(&pool)
        .await
        .expect("opt");

        // Mutate the still-blocked parked task: one new work_item event bumps its
        // lifetime event count past the stamped epoch. `set_effort` is
        // status-agnostic, so the task stays `blocked`.
        crate::repo::set_effort(&pool, &task, crate::domain::Effort::M)
            .await
            .expect("set_effort");

        let err = resolve_open_question(&pool, &q, &opt, Some("human"))
            .await
            .expect_err("stale resolution must reject");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // The reject left the question OPEN and the task still parked (no write).
        let status = sqlx::query_scalar::<_, Option<String>>(
            "SELECT status FROM open_questions WHERE id = ?1",
        )
        .bind(&q)
        .fetch_one(&pool)
        .await
        .expect("status read");
        assert_eq!(status.as_deref(), Some("open"), "stale reject does not resolve");
        assert_eq!(item_status(&pool, &task).await, "blocked", "task stays parked");

        // --- (b) CLEAN: epoch matches, task still parked → normal resolve. ----
        let task2 = create_work_item(&pool, "task", Some(&story), "T2", None)
            .await
            .expect("task2")
            .to_string();
        let q2 = escalate_decision_and_park_task(&pool, &story, &task2, "which2?", &["X"])
            .await
            .expect("escalate2")
            .to_string();
        let opt2 = sqlx::query_scalar::<_, String>(
            "SELECT id FROM question_options WHERE question_id = ?1 ORDER BY seq LIMIT 1",
        )
        .bind(&q2)
        .fetch_one(&pool)
        .await
        .expect("opt2");
        resolve_open_question(&pool, &q2, &opt2, Some("human"))
            .await
            .expect("clean resolve");
        assert_eq!(item_status(&pool, &task2).await, "todo", "clean resolve unblocks");
        let after_close2 = sqlx::query_scalar::<_, Option<String>>(
            "SELECT resolved_after_close FROM open_questions WHERE id = ?1",
        )
        .bind(&q2)
        .fetch_one(&pool)
        .await
        .expect("after_close2 read");
        assert!(after_close2.is_none(), "a live-parked resolve leaves resolved_after_close NULL");

        // --- (c) AFTER-CLOSE: no parked consumer remains → marker stamped. ----
        let task3 = create_work_item(&pool, "task", Some(&story), "T3", None)
            .await
            .expect("task3")
            .to_string();
        let q3 = escalate_decision_and_park_task(&pool, &story, &task3, "which3?", &["Y"])
            .await
            .expect("escalate3")
            .to_string();
        let opt3 = sqlx::query_scalar::<_, String>(
            "SELECT id FROM question_options WHERE question_id = ?1 ORDER BY seq LIMIT 1",
        )
        .bind(&q3)
        .fetch_one(&pool)
        .await
        .expect("opt3");
        // The run clean-closed: the parked task is cancelled away before the
        // human answers, so no live consumer remains to re-initiate.
        sqlx::query("UPDATE work_items SET status = 'cancelled' WHERE id = ?1")
            .bind(&task3)
            .execute(&pool)
            .await
            .expect("cancel task3");
        resolve_open_question(&pool, &q3, &opt3, Some("human"))
            .await
            .expect("after-close resolve still succeeds");
        let after_close3 = sqlx::query_scalar::<_, Option<String>>(
            "SELECT resolved_after_close FROM open_questions WHERE id = ?1",
        )
        .bind(&q3)
        .fetch_one(&pool)
        .await
        .expect("after_close3 read");
        assert!(
            after_close3.is_some(),
            "a resolution with no live parked consumer stamps resolved_after_close (seq28)"
        );
    }
}
