//! Acceptance-criteria CRUD (R2 carve): `add_acceptance_criterion`, the
//! `check`/`uncheck` toggles (a check appends an immutable `verification`
//! activity row), and the hard-`remove`. The `acceptance_criterion_work_item`
//! owner-resolver is private to this cluster (the check/uncheck/remove paths
//! attribute the owning item).
//!
//! The cross-cluster substrate (`work_item_kind`, `check_plan_field_len`) lives
//! in `repo/shared.rs` and is reached via `use super::*`; the event-outbox
//! writer comes from `super::events`. `pub use acceptance_criteria::*` in
//! `repo/mod.rs` PRESERVES every `pub` fn's `crate::repo::*` path.

use uuid::Uuid;

use super::*;
use super::events::record_event;
use crate::args;
use crate::db::DbClient;
use crate::error::AppError;

/// Append ONE `acceptance_criteria` row under the single-mutation-path
/// discipline (migration 0003, mirroring [`append_activity`]). `seq` is
/// allocated `MAX(seq)+1` per work item WITHIN the transaction; the
/// `UNIQUE(work_item_id, seq)` constraint surfaces a race as a constraint
/// violation. The work item must exist (`NotFound` otherwise). Event
/// `work_item.acceptance_criterion_added`. Returns the new criterion id.
pub async fn add_acceptance_criterion(
    db: &impl DbClient,
    work_item_id: &str,
    text: &str,
) -> Result<Uuid, AppError> {
    // Verify the work item exists first (NotFound, not a dangling-FK 500).
    let _ = work_item_kind(db, work_item_id).await?;

    // R43: reject a blank criterion (a whitespace-only close-criterion would
    // vacuously satisfy the story-create ≥1-criterion gate) and cap storage
    // amplification at the shared 64-KiB plan-field limit.
    if text.trim().is_empty() {
        return Err(AppError::Validation(
            "acceptance-criterion text must be non-empty".into(),
        ));
    }
    check_plan_field_len("acceptance-criterion text", text)?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM acceptance_criteria WHERE work_item_id = $1",
        args![work_item_id.to_owned()],
    )
    .await?;

    tx.execute(
        "INSERT INTO acceptance_criteria (id, work_item_id, seq, text) VALUES ($1, $2, $3, $4)",
        args![id_str.clone(), work_item_id.to_owned(), seq, text.to_owned()],
    )
    .await?;

    let payload = serde_json::json!({ "criterion_id": id_str, "seq": seq });
    record_event(
        tx.as_mut(),
        "work_item",
        work_item_id,
        "work_item.acceptance_criterion_added",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(id)
}

/// Read an acceptance criterion's owning `work_item_id`, erroring `NotFound` if
/// the criterion id has no row. Used by the check/uncheck paths to attribute the
/// owning item (for the audit-activity append and the event aggregate).
async fn acceptance_criterion_work_item(
    db: &impl DbClient,
    id: &str,
) -> Result<String, AppError> {
    crate::db::scalar_opt::<String>(
        db,
        "SELECT work_item_id FROM acceptance_criteria WHERE id = $1",
        args![id.to_owned()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("acceptance_criterion '{id}' not found")))
}

/// Check an acceptance criterion (migration 0003): set `checked=1`,
/// `checked_at=CURRENT_TIMESTAMP`, `checked_by`, AND append a `verification`
/// `work_item_activity` row for the owning work item (state-vs-immutable-audit,
/// per the plan's acceptance-criteria research note) — all in ONE transaction
/// with ONE event. The owning work_item_id is read first (`NotFound` if the
/// criterion is absent). Event `work_item.acceptance_criterion_checked`.
pub async fn check_acceptance_criterion(
    db: &impl DbClient,
    id: &str,
    by: Option<&str>,
) -> Result<(), AppError> {
    let work_item_id = acceptance_criterion_work_item(db, id).await?;

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "UPDATE acceptance_criteria \
             SET checked = 1, checked_at = CURRENT_TIMESTAMP, checked_by = $2 \
             WHERE id = $1",
            args![id.to_owned(), by.map(str::to_owned)],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("acceptance_criterion '{id}' not found")));
    }

    // Append the immutable verification-audit activity row for the owning item.
    // seq is allocated MAX(seq)+1 within this same tx.
    let activity_id = Uuid::now_v7().to_string();
    let act_seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM work_item_activity WHERE work_item_id = $1",
        args![work_item_id.clone()],
    )
    .await?;
    let summary = format!("acceptance criterion {id} checked");
    tx.execute(
        "INSERT INTO work_item_activity (id, work_item_id, seq, entry_kind, author, summary) \
         VALUES ($1, $2, $3, 'verification', $4, $5)",
        args![
            activity_id,
            work_item_id.clone(),
            act_seq,
            by.map(str::to_owned),
            summary
        ],
    )
    .await?;

    let payload = serde_json::json!({ "criterion_id": id, "checked": true });
    record_event(
        tx.as_mut(),
        "work_item",
        &work_item_id,
        "work_item.acceptance_criterion_checked",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Uncheck an acceptance criterion (migration 0003): clear `checked`,
/// `checked_at`, `checked_by`. One event. `NotFound` via `rows_affected()==0`.
/// (No audit-activity row — un-checking is a correction, not a verification.)
pub async fn uncheck_acceptance_criterion(db: &impl DbClient, id: &str) -> Result<(), AppError> {
    let work_item_id = acceptance_criterion_work_item(db, id).await?;

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "UPDATE acceptance_criteria \
             SET checked = 0, checked_at = NULL, checked_by = NULL \
             WHERE id = $1",
            args![id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("acceptance_criterion '{id}' not found")));
    }

    let payload = serde_json::json!({ "criterion_id": id, "checked": false });
    record_event(
        tx.as_mut(),
        "work_item",
        &work_item_id,
        "work_item.acceptance_criterion_unchecked",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Hard-delete an acceptance criterion (migration 0003): criteria have no
/// independent export identity, so a removal is a hard DELETE. One event.
/// `NotFound` via `rows_affected()==0`.
pub async fn remove_acceptance_criterion(db: &impl DbClient, id: &str) -> Result<(), AppError> {
    // Resolve the owning item first so the event aggregate is the work_item
    // (and so an absent criterion is NotFound before any write).
    let work_item_id = acceptance_criterion_work_item(db, id).await?;

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "DELETE FROM acceptance_criteria WHERE id = $1",
            args![id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("acceptance_criterion '{id}' not found")));
    }

    let payload = serde_json::json!({ "criterion_id": id, "removed": true });
    record_event(
        tx.as_mut(),
        "work_item",
        &work_item_id,
        "work_item.acceptance_criterion_removed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::domain::{ClosureGate, Status, UpdateWorkItemRequest};
    use crate::repo::test_support::*;

    /// `get_work_item_detail` folds the acceptance_criteria; an add emits +1
    /// event and the criterion starts unchecked.
    #[tokio::test]
    async fn acceptance_criteria_fold_and_add_event() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        let ev_before = count_events(&pool).await;
        // The seed adds one epic close-criterion (migration-0010 story gate), so
        // count the delta rather than an absolute global criterion count.
        let crit_before = count_criteria(&pool).await;

        add_acceptance_criterion(&pool, &task, "must build").await.expect("ac1");
        add_acceptance_criterion(&pool, &task, "must test").await.expect("ac2");

        assert_eq!(count_criteria(&pool).await, crit_before + 2);
        assert_eq!(count_events(&pool).await, ev_before + 2, "+1 event per add");

        let detail = get_work_item_detail(&pool, &task).await.expect("detail");
        assert_eq!(detail.acceptance_criteria.len(), 2, "detail folds criteria");
        assert_eq!(detail.acceptance_criteria[0].seq, 1);
        assert_eq!(detail.acceptance_criteria[1].seq, 2, "monotonic seq");
        assert_eq!(detail.acceptance_criteria[0].checked, 0, "starts unchecked");
    }

    /// Checking a criterion flips its state, appends exactly one `verification`
    /// activity row, and records exactly one event.
    #[tokio::test]
    async fn check_criterion_writes_activity_and_one_event() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        let crit = add_acceptance_criterion(&pool, &task, "must build")
            .await
            .expect("ac")
            .to_string();

        let ev_before = count_events(&pool).await;
        let act_before = count_activity(&pool).await;

        check_acceptance_criterion(&pool, &crit, Some("alice"))
            .await
            .expect("check");

        assert_eq!(count_events(&pool).await, ev_before + 1, "exactly one event");
        assert_eq!(count_activity(&pool).await, act_before + 1, "+1 activity");

        let detail = get_work_item_detail(&pool, &task).await.expect("detail");
        assert_eq!(detail.acceptance_criteria[0].checked, 1, "criterion flipped");
        assert_eq!(detail.acceptance_criteria[0].checked_by.as_deref(), Some("alice"));
        // The appended activity is a verification entry.
        let verif = detail.activity.iter().find(|a| a.entry_kind == "verification");
        assert!(verif.is_some(), "a verification activity row was appended");

        // Uncheck clears state (no extra activity row, one event).
        let ev2 = count_events(&pool).await;
        let act2 = count_activity(&pool).await;
        uncheck_acceptance_criterion(&pool, &crit).await.expect("uncheck");
        assert_eq!(count_events(&pool).await, ev2 + 1, "uncheck: one event");
        assert_eq!(count_activity(&pool).await, act2, "uncheck: no new activity");
        let detail = get_work_item_detail(&pool, &task).await.expect("detail");
        assert_eq!(detail.acceptance_criteria[0].checked, 0, "unchecked");
        assert!(detail.acceptance_criteria[0].checked_by.is_none(), "checked_by cleared");
    }

    /// A `hard` story blocks task→done while a criterion is unchecked, and allows
    /// it once all are checked — across BOTH gated paths (update_work_item_status
    /// and the generic update_work_item PATCH).
    #[tokio::test]
    async fn hard_gate_blocks_then_allows_task_done() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        set_closure_gate(&pool, &story, ClosureGate::Hard).await.expect("hard gate");

        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        let crit = add_acceptance_criterion(&pool, &task, "must build")
            .await
            .expect("ac")
            .to_string();

        // Blocked while unchecked (status path).
        let err = update_work_item_status(&pool, &task, "done")
            .await
            .expect_err("hard gate blocks done");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // Blocked while unchecked (generic PATCH path).
        let patch_done = UpdateWorkItemRequest {
            title: None,
            body: None,
            status: Some(Status::Done),
            position: None,
            attributes: None,
        };
        let err = update_work_item(&pool, &task, &patch_done)
            .await
            .expect_err("PATCH→done also gated");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // Check the criterion → done now allowed.
        check_acceptance_criterion(&pool, &crit, None).await.expect("check");
        update_work_item_status(&pool, &task, "done").await.expect("done allowed once checked");
        let detail = get_work_item_detail(&pool, &task).await.expect("detail");
        assert_eq!(detail.item.status, "done");
    }

    /// (R18) Multi-criterion hard gate: with TWO acceptance criteria, checking
    /// only ONE still blocks task→done; checking BOTH allows it. This catches a
    /// count-total-vs-count-unchecked bug that a single-criterion test misses
    /// (a "count == total" gate would wrongly allow done after the first check).
    #[tokio::test]
    async fn hard_gate_multi_criterion_partial_check_still_blocks() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        set_closure_gate(&pool, &story, ClosureGate::Hard).await.expect("hard gate");

        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        let crit_a = add_acceptance_criterion(&pool, &task, "must build")
            .await
            .expect("ac1")
            .to_string();
        let crit_b = add_acceptance_criterion(&pool, &task, "must test")
            .await
            .expect("ac2")
            .to_string();

        // Zero checked → blocked.
        let err = update_work_item_status(&pool, &task, "done")
            .await
            .expect_err("blocked with both unchecked");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // Check only ONE of the two → STILL blocked (the partial-check case).
        check_acceptance_criterion(&pool, &crit_a, None).await.expect("check first");
        let err = update_work_item_status(&pool, &task, "done")
            .await
            .expect_err("one-of-two checked must still block");
        assert!(
            matches!(err, AppError::Validation(_)),
            "partial check must still block done, got {err:?}"
        );

        // Check the SECOND → now allowed.
        check_acceptance_criterion(&pool, &crit_b, None).await.expect("check second");
        update_work_item_status(&pool, &task, "done")
            .await
            .expect("done allowed once BOTH criteria checked");
        let detail = get_work_item_detail(&pool, &task).await.expect("detail");
        assert_eq!(detail.item.status, "done");
    }

    /// A `soft` story (the default — no closure_gate set) allows task→done even
    /// with an unchecked criterion.
    #[tokio::test]
    async fn soft_gate_allows_task_done_with_unchecked() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        // No set_closure_gate call ⇒ closure_gate is NULL (treated as soft).
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        add_acceptance_criterion(&pool, &task, "unchecked criterion")
            .await
            .expect("ac");

        update_work_item_status(&pool, &task, "done")
            .await
            .expect("soft gate allows done with unchecked criteria");
        let detail = get_work_item_detail(&pool, &task).await.expect("detail");
        assert_eq!(detail.item.status, "done");
    }
}
