//! Risks (migration 0005, R4 carve). Mirror the `research_notes` CRUD:
//! append-with-seq, partial set-or-leave update, supersession by self-FK, hard
//! remove. Severity is a closed enum CHECK-constrained at the DB layer
//! (low|medium|high|critical); we validate it here so an invalid value surfaces
//! as `Validation` (→ 422) rather than a constraint-violation 500.
//!
//! Events are routed to the owning work-item's `work_item` aggregate (NOT a
//! fresh `risk` aggregate), because `export.rs`'s drain dispatch only re-renders
//! `work_item` aggregates; a stand-alone aggregate type would never reach the
//! git-export snapshot.
//!
//! The shared substrate these compose on (`enum_to_str`, `work_item_kind`) lives
//! in `repo/shared.rs` and is reached via `use super::*`; the event-outbox writer
//! comes from `super::events`.
//!
//! `pub use risks::*` in `repo/mod.rs` PRESERVES the public surface — every
//! `pub` fn here stays reachable at its existing `crate::repo::*` path (the HTTP
//! handlers / MCP tools / importer call them by path and are unchanged). The
//! domain types named in the signatures are imported explicitly from `crate::*`
//! (a `use super::*` glob does NOT carry super's private `use` imports).

use serde_json::Value;
use uuid::Uuid;

use super::*;
use super::events::record_event;
use crate::args;
use crate::db::DbClient;
use crate::domain::{RiskSeverity, RiskPatch};
use crate::error::AppError;

/// Render the canonical wire spelling of a `RiskSeverity` for storage. Mirrors
/// `enum_to_str` but takes a typed enum so callers cannot fabricate an invalid
/// value at the call site. The `&str` callers (e.g. `add_risk`) go through
/// `validate_risk_severity_str` to project a raw string into this enum first.
fn risk_severity_str(s: RiskSeverity) -> String {
    enum_to_str(s)
}

/// Validate a raw severity string against the closed [`RiskSeverity`] enum.
/// Surfaces a clean `Validation` (→ 422) on an unknown value, BEFORE the DB
/// CHECK constraint would otherwise fire as a `Db` 500. The canonical wire
/// spelling (lowercase) is returned for storage.
fn validate_risk_severity_str(s: &str) -> Result<String, AppError> {
    serde_json::from_value::<RiskSeverity>(Value::String(s.to_owned()))
        .map(risk_severity_str)
        .map_err(|_| {
            AppError::Validation(format!(
                "unknown risk severity '{s}' (expected one of low, medium, high, critical)"
            ))
        })
}

/// Append ONE `risks` row under the single-mutation-path discipline (migration
/// 0005). Mirrors [`add_research_note`]: `seq = MAX+1` per work item allocated
/// inside the tx, work item must exist (`NotFound` otherwise), severity validated
/// against the closed [`RiskSeverity`] enum BEFORE the write so an unknown value
/// is a clean 422 (not a `Db` 500 from the CHECK constraint). Event
/// `risk.added` routed to the owning work-item's `work_item` aggregate so
/// `export.rs` re-renders. Returns the new risk id.
pub async fn add_risk(
    db: &impl DbClient,
    work_item_id: &str,
    summary: &str,
    body: Option<&str>,
    rationale: Option<&str>,
    severity: &str,
    mitigation: Option<&str>,
) -> Result<Uuid, AppError> {
    let severity = validate_risk_severity_str(severity)?;
    // Verify the work item exists first (NotFound, not a dangling-FK 500).
    let _ = work_item_kind(db, work_item_id).await?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM risks WHERE work_item_id = $1",
        args![work_item_id.to_owned()],
    )
    .await?;

    tx.execute(
        r#"
        INSERT INTO risks
            (id, work_item_id, seq, summary, body, rationale, severity, mitigation)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        args![
            id_str.clone(),
            work_item_id.to_owned(),
            seq,
            summary.to_owned(),
            body.map(str::to_owned),
            rationale.map(str::to_owned),
            severity.to_owned(),
            mitigation.map(str::to_owned),
        ],
    )
    .await?;

    let payload = serde_json::json!({
        "risk_id": id_str,
        "seq": seq,
        "severity": severity,
    });
    record_event(tx.as_mut(), "work_item", work_item_id, "risk.added", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Read a risk's owning `work_item_id`, erroring `NotFound` if the risk id has
/// no row. Mirrors [`research_note_work_item`] — the update / supersede /
/// remove paths all need the owning aggregate id for the event routing.
async fn risk_work_item(db: &impl DbClient, id: &str) -> Result<String, AppError> {
    crate::db::scalar_opt::<String>(
        db,
        "SELECT work_item_id FROM risks WHERE id = $1",
        args![id.to_owned()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("risk '{id}' not found")))
}

/// Partial set-or-leave update of a risk's curatable fields (migration 0005):
/// `summary`/`body`/`rationale`/`severity`/`mitigation` via `COALESCE(?, col)`.
/// The typed [`RiskSeverity`] is rendered to its wire form before the COALESCE
/// bind. Mirrors [`update_research_note`]. `NotFound` via `rows_affected()==0`;
/// one event `risk.updated`.
pub async fn update_risk(
    db: &impl DbClient,
    id: &str,
    patch: &RiskPatch,
) -> Result<(), AppError> {
    let work_item_id = risk_work_item(db, id).await?;
    let severity_str: Option<String> = patch.severity.map(risk_severity_str);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE risks
        SET summary    = COALESCE($2, summary),
            body       = COALESCE($3, body),
            rationale  = COALESCE($4, rationale),
            severity   = COALESCE($5, severity),
            mitigation = COALESCE($6, mitigation)
        WHERE id = $1
        "#,
            args![
                id.to_owned(),
                patch.summary.clone(),
                patch.body.clone(),
                patch.rationale.clone(),
                severity_str.clone(),
                patch.mitigation.clone(),
            ],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("risk '{id}' not found")));
    }

    let payload = serde_json::json!({
        "risk_id": id,
        "severity": severity_str,
    });
    record_event(tx.as_mut(), "work_item", &work_item_id, "risk.updated", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Supersede a risk (migration 0005): insert a NEW risk row under the same
/// work item, then set `superseded_by = new_id` on the OLD row so it drops out
/// of the live `list_risks` fold. Both writes share ONE transaction and emit
/// EXACTLY ONE `risk.superseded` event (NOT a separate `risk.added` for the
/// new row — supersession is one logical write, mirroring the research-note
/// supersession discipline in [`supersede_research_note`]). Returns the new id.
#[allow(clippy::too_many_arguments)]
pub async fn supersede_risk(
    db: &impl DbClient,
    work_item_id: &str,
    old_id: &str,
    new_summary: &str,
    new_body: Option<&str>,
    new_rationale: Option<&str>,
    new_severity: &str,
    new_mitigation: Option<&str>,
) -> Result<Uuid, AppError> {
    let severity = validate_risk_severity_str(new_severity)?;
    // Verify the old risk belongs to the named work item; NotFound otherwise.
    let actual_wi = risk_work_item(db, old_id).await?;
    if actual_wi != work_item_id {
        return Err(AppError::Validation(format!(
            "risk '{old_id}' belongs to work_item '{actual_wi}', not '{work_item_id}'"
        )));
    }

    let new_id = Uuid::now_v7();
    let new_id_str = new_id.to_string();

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM risks WHERE work_item_id = $1",
        args![work_item_id.to_owned()],
    )
    .await?;

    tx.execute(
        r#"
        INSERT INTO risks
            (id, work_item_id, seq, summary, body, rationale, severity, mitigation)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        args![
            new_id_str.clone(),
            work_item_id.to_owned(),
            seq,
            new_summary.to_owned(),
            new_body.map(str::to_owned),
            new_rationale.map(str::to_owned),
            severity.clone(),
            new_mitigation.map(str::to_owned),
        ],
    )
    .await?;

    let affected = tx
        .execute(
            "UPDATE risks SET superseded_by = $2 WHERE id = $1",
            args![old_id.to_owned(), new_id_str.clone()],
        )
        .await?;

    if affected == 0 {
        // Concurrent delete between `risk_work_item` read and the UPDATE; the
        // tx drops → ROLLBACK so the INSERT above does not leak.
        return Err(AppError::NotFound(format!("risk '{old_id}' not found")));
    }

    let payload = serde_json::json!({
        "old_id": old_id,
        "new_id": new_id_str,
        "seq": seq,
        "severity": severity,
    });
    record_event(tx.as_mut(), "work_item", work_item_id, "risk.superseded", payload).await?;

    tx.commit().await?;
    Ok(new_id)
}

/// Hard-delete a risk under the single-mutation-path discipline. Risks have no
/// independent export identity (they fold into the owning work-item's TOML), so
/// removal is a hard DELETE. `NotFound` via `rows_affected()==0`. Event
/// `risk.removed` on the owning work-item's aggregate.
pub async fn remove_risk(db: &impl DbClient, id: &str) -> Result<(), AppError> {
    let work_item_id = risk_work_item(db, id).await?;

    let mut tx = db.begin().await?;

    let affected = tx
        .execute("DELETE FROM risks WHERE id = $1", args![id.to_owned()])
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("risk '{id}' not found")));
    }

    let payload = serde_json::json!({ "risk_id": id, "removed": true });
    record_event(tx.as_mut(), "work_item", &work_item_id, "risk.removed", payload).await?;

    tx.commit().await?;
    Ok(())
}
