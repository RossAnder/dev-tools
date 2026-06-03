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
/// committed/rolled-back atomically with the domain write. Every caller passes
/// `&mut tx` where `tx: Transaction<'_, Sqlite>` came from
/// [`crate::db::begin_write`]; that reference unsizes to `&mut dyn DbTx` via the
/// `impl DbTx for Transaction<'_, Sqlite>` blanket coercion, so the ~100 callers
/// need no changes.
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

    Ok(())
}

/// Append ONE export-INERT `events` row (R19). The batch/domain write paths
/// (`create_work_items`, `add_findings`, `batch_update_findings`,
/// `add_tasks_to_sprint`, `record_finding_decision`, and the run/sprint
/// creators) record a single coarse event whose `aggregate_type` MUST be one of
/// the inert kinds (`run`/`sprint`/`finding`/`batch`) and MUST NEVER be
/// `"work_item"`: the git-export drain (`export.rs`) materialises ONLY
/// `aggregate_type="work_item"` events, so a `"work_item"`-typed batch/domain
/// event would wrongly re-render its aggregate (R-B4).
///
/// This helper centralises that invariant — previously hand-repeated as a
/// comment at six call sites — behind a HARD runtime guard: an `aggregate_type`
/// of `"work_item"` is rejected with [`AppError::Validation`] (a programmer
/// error caught before the row is written) rather than silently mis-routed.
/// Otherwise it delegates verbatim to [`record_event`].
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
             aggregate_type (run/sprint/finding/batch)"
        )));
    }
    record_event(tx, aggregate_type, aggregate_id, event_type, payload).await
}
