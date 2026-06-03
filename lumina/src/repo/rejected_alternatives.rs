//! Rejected alternatives (migration 0005, R4 carve). Same shape as `risks`
//! minus severity; `confidence` is free TEXT (matches `research_notes.confidence`
//! — validated in the repo, NOT a DB CHECK).
//!
//! Events are routed to the owning work-item's `work_item` aggregate (NOT a
//! fresh `rejected_alternative` aggregate), because `export.rs`'s drain dispatch
//! only re-renders `work_item` aggregates; a stand-alone aggregate type would
//! never reach the git-export snapshot.
//!
//! The shared substrate these compose on (`work_item_kind`) lives in
//! `repo/shared.rs` and is reached via `use super::*`; the event-outbox writer
//! comes from `super::events`.
//!
//! `pub use rejected_alternatives::*` in `repo/mod.rs` PRESERVES the public
//! surface — every `pub` fn here stays reachable at its existing `crate::repo::*`
//! path (the HTTP handlers / MCP tools / importer call them by path and are
//! unchanged). The domain types named in the signatures are imported explicitly
//! from `crate::*` (a `use super::*` glob does NOT carry super's private `use`
//! imports).

use uuid::Uuid;

use super::*;
use super::events::record_event;
use crate::args;
use crate::db::DbClient;
use crate::domain::AlternativePatch;
use crate::error::AppError;

/// Append ONE `rejected_alternatives` row under the single-mutation-path
/// discipline (migration 0005). Mirrors [`add_risk`] minus the severity
/// validation: `confidence` is free TEXT (validated nowhere at the DB; mirrors
/// `research_notes.confidence`). Event `rejected_alternative.added`.
pub async fn add_rejected_alternative(
    db: &impl DbClient,
    work_item_id: &str,
    summary: &str,
    body: Option<&str>,
    rationale: Option<&str>,
    confidence: Option<&str>,
) -> Result<Uuid, AppError> {
    // Verify the work item exists first (NotFound, not a dangling-FK 500).
    let _ = work_item_kind(db, work_item_id).await?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM rejected_alternatives WHERE work_item_id = $1",
        args![work_item_id.to_owned()],
    )
    .await?;

    tx.execute(
        r#"
        INSERT INTO rejected_alternatives
            (id, work_item_id, seq, summary, body, rationale, confidence)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        args![
            id_str.clone(),
            work_item_id.to_owned(),
            seq,
            summary.to_owned(),
            body.map(str::to_owned),
            rationale.map(str::to_owned),
            confidence.map(str::to_owned),
        ],
    )
    .await?;

    let payload = serde_json::json!({
        "alternative_id": id_str,
        "seq": seq,
    });
    record_event(
        tx.as_mut(),
        "work_item",
        work_item_id,
        "rejected_alternative.added",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(id)
}

/// Read a rejected-alternative's owning `work_item_id`, erroring `NotFound` if
/// the id has no row. Mirrors [`risk_work_item`].
async fn rejected_alternative_work_item(
    db: &impl DbClient,
    id: &str,
) -> Result<String, AppError> {
    crate::db::scalar_opt::<String>(
        db,
        "SELECT work_item_id FROM rejected_alternatives WHERE id = $1",
        args![id.to_owned()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("rejected_alternative '{id}' not found")))
}

/// Partial set-or-leave update of a rejected-alternative's curatable fields
/// (migration 0005). Mirrors [`update_risk`] minus severity; `confidence` is
/// free TEXT, no enum projection. Event `rejected_alternative.updated`.
pub async fn update_rejected_alternative(
    db: &impl DbClient,
    id: &str,
    patch: &AlternativePatch,
) -> Result<(), AppError> {
    let work_item_id = rejected_alternative_work_item(db, id).await?;

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE rejected_alternatives
        SET summary    = COALESCE($2, summary),
            body       = COALESCE($3, body),
            rationale  = COALESCE($4, rationale),
            confidence = COALESCE($5, confidence)
        WHERE id = $1
        "#,
            args![
                id.to_owned(),
                patch.summary.clone(),
                patch.body.clone(),
                patch.rationale.clone(),
                patch.confidence.clone(),
            ],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("rejected_alternative '{id}' not found")));
    }

    let payload = serde_json::json!({ "alternative_id": id });
    record_event(
        tx.as_mut(),
        "work_item",
        &work_item_id,
        "rejected_alternative.updated",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Supersede a rejected alternative (migration 0005): insert a NEW row under
/// the same work item, then point the OLD row at it via `superseded_by`.
/// Mirrors [`supersede_risk`]; one transaction, one event
/// `rejected_alternative.superseded`. Returns the new id.
pub async fn supersede_rejected_alternative(
    db: &impl DbClient,
    work_item_id: &str,
    old_id: &str,
    new_summary: &str,
    new_body: Option<&str>,
    new_rationale: Option<&str>,
    new_confidence: Option<&str>,
) -> Result<Uuid, AppError> {
    let actual_wi = rejected_alternative_work_item(db, old_id).await?;
    if actual_wi != work_item_id {
        return Err(AppError::Validation(format!(
            "rejected_alternative '{old_id}' belongs to work_item '{actual_wi}', \
             not '{work_item_id}'"
        )));
    }

    let new_id = Uuid::now_v7();
    let new_id_str = new_id.to_string();

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM rejected_alternatives WHERE work_item_id = $1",
        args![work_item_id.to_owned()],
    )
    .await?;

    tx.execute(
        r#"
        INSERT INTO rejected_alternatives
            (id, work_item_id, seq, summary, body, rationale, confidence)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        args![
            new_id_str.clone(),
            work_item_id.to_owned(),
            seq,
            new_summary.to_owned(),
            new_body.map(str::to_owned),
            new_rationale.map(str::to_owned),
            new_confidence.map(str::to_owned),
        ],
    )
    .await?;

    let affected = tx
        .execute(
            "UPDATE rejected_alternatives SET superseded_by = $2 WHERE id = $1",
            args![old_id.to_owned(), new_id_str.clone()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!(
            "rejected_alternative '{old_id}' not found"
        )));
    }

    let payload = serde_json::json!({
        "old_id": old_id,
        "new_id": new_id_str,
        "seq": seq,
    });
    record_event(
        tx.as_mut(),
        "work_item",
        work_item_id,
        "rejected_alternative.superseded",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(new_id)
}

/// Hard-delete a rejected alternative under the single-mutation-path discipline.
/// `NotFound` via `rows_affected()==0`; one event `rejected_alternative.removed`.
pub async fn remove_rejected_alternative(db: &impl DbClient, id: &str) -> Result<(), AppError> {
    let work_item_id = rejected_alternative_work_item(db, id).await?;

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "DELETE FROM rejected_alternatives WHERE id = $1",
            args![id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("rejected_alternative '{id}' not found")));
    }

    let payload = serde_json::json!({ "alternative_id": id, "removed": true });
    record_event(
        tx.as_mut(),
        "work_item",
        &work_item_id,
        "rejected_alternative.removed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}
