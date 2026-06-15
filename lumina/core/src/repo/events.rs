//! Event-outbox writers shared by every repo mutator.
//!
//! [`record_event`] appends ONE `events` row inside an in-flight transaction;
//! [`record_inert_event`] is the export-inert variant used by the batch/domain
//! write paths. Both are `pub(crate)` so the mutator clusters that remain in
//! `repo/mod.rs` (and the other repo siblings) keep calling them unqualified.

use uuid::Uuid;

use crate::error::AppError;

/// Append ONE `events` row inside an in-flight transaction. Called by every
/// mutation; no domain write may bypass it. `id` is a fresh UUIDv7 (TEXT);
/// `payload` is serialised to a JSON string; `exported_at` is left NULL so the
/// git-export materialiser (Task 6) drains it on its next tick.
///
/// Takes a `&mut dyn DbTx` (the backend-erased in-flight transaction, not the
/// pool) precisely so the event row shares the caller's transaction and is
/// committed/rolled-back atomically with the domain write. Mutators hold a
/// `Box<dyn DbTx>` obtained from `DbClient::begin` (which returns the
/// `NotifyingTx` wrapper — see `db/erased.rs`) and pass it here as
/// `&mut dyn DbTx` (e.g. via `tx.as_mut()`); there are zero live
/// `crate::db::begin_write` call sites in `repo/`.
///
/// As a side effect, this is the single place every domain write funnels
/// through, so it also buffers ONE [`crate::notify::ChangeNotification`] on the
/// transaction via [`crate::db::DbTx::note_change`] — flushed to the
/// process-wide notify bus by `NotifyingTx::commit` AFTER the commit succeeds
/// (a rolled-back transaction publishes nothing).
pub(crate) async fn record_event(
    tx: &mut dyn crate::db::DbTx,
    aggregate_type: &str,
    aggregate_id: &str,
    event_type: &str,
    payload: serde_json::Value,
) -> Result<(), AppError> {
    let event_id = Uuid::now_v7().to_string();
    let payload_str = serde_json::to_string(&payload).map_err(|e| AppError::Other(e.into()))?;

    // Runtime trait call through the object-safe `DbTx::execute` (placeholders
    // are `$N`, args are owned/`'static`: the borrowed `&str` params are
    // `.to_owned()`'d before binding; `event_id`/`payload_str` are already
    // owned `String`). The returned affected-row count is ignored.
    tx.execute(
        r#"
        INSERT INTO events (id, aggregate_type, aggregate_id, event_type, payload)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        crate::args![
            event_id,
            aggregate_type.to_owned(),
            aggregate_id.to_owned(),
            event_type.to_owned(),
            payload_str
        ],
    )
    .await?;

    // Buffer the post-commit change notification on the transaction (sync, no
    // await). `NotifyingTx` flushes it to the notify bus only after a
    // successful commit; a raw `Transaction<Sqlite>` (begin_write path) takes
    // the default no-op.
    tx.note_change(crate::notify::ChangeNotification::new(
        aggregate_type,
        aggregate_id,
        event_type,
    ));

    Ok(())
}

/// Append ONE export-INERT `events` row (R19). The batch/domain write paths
/// (`create_work_items`, `add_findings`, `batch_update_findings`,
/// `add_tasks_to_sprint`, `record_finding_decision`, the run/sprint creators,
/// the worktree/task-commit substrate, the task-files writers, and the harness
/// session-corpus ingest) record a single coarse event whose `aggregate_type`
/// MUST be one of the inert kinds
/// (`run`/`sprint`/`finding`/`batch`/`session`/`worktree`/`task_files`) and MUST
/// NEVER be `"work_item"`: the git-export drain (`export.rs`) materialises ONLY
/// `aggregate_type="work_item"` events, so a `"work_item"`-typed batch/domain
/// event would wrongly re-render its aggregate (R-B4).
///
/// This helper centralises that invariant — previously hand-repeated as a
/// comment at six call sites — behind a HARD runtime guard: an `aggregate_type`
/// of `"work_item"` is rejected with [`AppError::Validation`] (a programmer
/// error caught before the row is written) rather than silently mis-routed.
/// Otherwise it delegates verbatim to [`record_event`] — any inert kind
/// (`run`/`sprint`/`finding`/`batch`/`session`/`worktree`/`task_files`) is
/// accepted.
pub(crate) async fn record_inert_event(
    tx: &mut dyn crate::db::DbTx,
    aggregate_type: &str,
    aggregate_id: &str,
    event_type: &str,
    payload: serde_json::Value,
) -> Result<(), AppError> {
    if aggregate_type == "work_item" {
        return Err(AppError::Validation(format!(
            "record_inert_event refuses aggregate_type=\"work_item\" for inert event \
             '{event_type}' (R-B4: the export drain would re-render it); use an inert \
             aggregate_type (run/sprint/finding/batch/session/worktree/task_files)"
        )));
    }
    record_event(tx, aggregate_type, aggregate_id, event_type, payload).await
}
