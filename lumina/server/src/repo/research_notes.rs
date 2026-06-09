//! Research-note CRUD — first-class records with confidence, accept/reject
//! state, and a `superseded_by` supersession chain (R3 carve, migration 0003).
//!
//! Mirror the acceptance-criteria/activity child-table idiom (seq = MAX+1 per
//! work item, one event per write). The shared substrate these compose on
//! (`enum_to_str`, `work_item_kind`) lives in `repo/shared.rs` and is reached via
//! `use super::*`; the event-outbox writer comes from `super::events`.
//!
//! `pub use research_notes::*` in `repo/mod.rs` PRESERVES the public surface —
//! every `pub` fn here stays reachable at its existing `crate::repo::*` path (the
//! HTTP handlers / MCP tools / importer call them by path and are unchanged). The
//! domain types named in the signatures are imported explicitly from `crate::*`
//! (a `use super::*` glob does NOT carry super's private `use` imports).

use uuid::Uuid;

use super::*;
use super::events::record_event;
use crate::args;
use crate::db::{DbClient, Scalar};
use crate::domain::{ResearchState, UpdateResearchNoteRequest};
use crate::error::AppError;

/// Append ONE `research_notes` row under the single-mutation-path discipline
/// (migration 0003). `seq` is `MAX(seq)+1` per work item WITHIN the transaction;
/// `state` defaults to `proposed`. The work item must exist (`NotFound`
/// otherwise). Event `work_item.research_note_added`. Returns the new note id.
#[allow(clippy::too_many_arguments)]
pub async fn add_research_note(
    db: &impl DbClient,
    work_item_id: &str,
    summary: &str,
    body: Option<&str>,
    confidence: Option<&str>,
    lens: Option<&str>,
    origin: Option<&str>,
) -> Result<Uuid, AppError> {
    // Verify the work item exists first (NotFound, not a dangling-FK 500).
    let _ = work_item_kind(db, work_item_id).await?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();
    // State defaults to `proposed` on create.
    let state = enum_to_str(ResearchState::Proposed);

    let mut tx = db.begin().await?;

    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM research_notes WHERE work_item_id = $1",
        args![work_item_id.to_owned()],
    )
    .await?;

    tx.execute(
        r#"
        INSERT INTO research_notes
            (id, work_item_id, seq, summary, body, confidence, state, lens, origin)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
        args![
            id_str.clone(),
            work_item_id.to_owned(),
            seq,
            summary.to_owned(),
            body.map(str::to_owned),
            confidence.map(str::to_owned),
            state,
            lens.map(str::to_owned),
            origin.map(str::to_owned),
        ],
    )
    .await?;

    let payload = serde_json::json!({ "note_id": id_str, "seq": seq });
    record_event(tx.as_mut(), "work_item", work_item_id, "work_item.research_note_added", payload)
        .await?;

    tx.commit().await?;
    Ok(id)
}

/// Read a research note's owning `work_item_id`, erroring `NotFound` if the note
/// id has no row. Used by the update/supersede paths to attribute the owning item
/// for the event aggregate.
async fn research_note_work_item(db: &impl DbClient, id: &str) -> Result<String, AppError> {
    crate::db::scalar_opt::<String>(
        db,
        "SELECT work_item_id FROM research_notes WHERE id = $1",
        args![id.to_owned()],
    )
    .await?
    .ok_or_else(|| AppError::NotFound(format!("research_note '{id}' not found")))
}

/// Partial set-or-leave update of a research note's curatable fields (migration
/// 0003): `confidence`/`state`/`rationale`/`lens` via `COALESCE(?, col)` (absent
/// ⇒ untouched). The typed `state` enum is rendered to its wire form. The owning
/// work_item_id is read first (`NotFound` if the note is absent). One event
/// `work_item.research_note_updated`.
pub async fn update_research_note(
    db: &impl DbClient,
    id: &str,
    req: &UpdateResearchNoteRequest,
) -> Result<(), AppError> {
    let work_item_id = research_note_work_item(db, id).await?;
    let state_str: Option<String> = req.state.map(enum_to_str);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"
        UPDATE research_notes
        SET confidence = COALESCE($2, confidence),
            state      = COALESCE($3, state),
            rationale  = COALESCE($4, rationale),
            lens       = COALESCE($5, lens)
        WHERE id = $1
        "#,
            args![
                id.to_owned(),
                req.confidence.clone(),
                state_str.clone(),
                req.rationale.clone(),
                req.lens.clone(),
            ],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("research_note '{id}' not found")));
    }

    let payload = serde_json::json!({ "note_id": id, "state": state_str });
    record_event(tx.as_mut(), "work_item", &work_item_id, "work_item.research_note_updated", payload)
        .await?;

    tx.commit().await?;
    Ok(())
}

/// Supersede a research note (migration 0003): set `superseded_by = new_id` on
/// the OLD note so it drops out of the live fold (`superseded_by IS NULL`). One
/// event `work_item.research_note_superseded`; `NotFound` via
/// `rows_affected()==0`. Mirrors [`supersede_finding`]. The `new_id` is
/// VALIDATED here (R7) — an absent `new_id` is a typed [`AppError::Validation`].
///
/// # Supersession / ON DELETE semantics (R14)
///
/// The supersession pointers — `findings.superseded_by`,
/// `research_notes.superseded_by` — and the open-question provenance pointers
/// `open_questions.prompting_finding_id` / `open_questions.prompting_note_id`
/// are currently declared `ON DELETE NO ACTION` in migration `0003`.
/// Supersession is a SOFT pointer (the superseded row is kept for the export
/// audit trail, never hard-deleted), so today nothing exercises the delete path.
/// A future hard-delete path SHOULD migrate these columns to `ON DELETE SET NULL`
/// to avoid a delete being blocked by — or leaving — a dangling pointer. Do NOT
/// edit the committed `0003_*.sql` to change this: that would alter its sqlx
/// migration checksum and break already-applied DBs (a new migration is the path).
pub async fn supersede_research_note(
    db: &impl DbClient,
    old_id: &str,
    new_id: &str,
) -> Result<(), AppError> {
    let work_item_id = research_note_work_item(db, old_id).await?;

    // Validate the superseding note exists (R7): clean 422 over a dangling-FK 500.
    let new_exists = db
        .query_opt::<Scalar<i64>>(
            "SELECT 1 FROM research_notes WHERE id = $1",
            args![new_id.to_owned()],
        )
        .await?
        .is_some();
    if !new_exists {
        return Err(AppError::Validation(format!(
            "superseding research_note '{new_id}' does not exist"
        )));
    }

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "UPDATE research_notes SET superseded_by = $2 WHERE id = $1",
            args![old_id.to_owned(), new_id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("research_note '{old_id}' not found")));
    }

    let payload = serde_json::json!({ "superseded_by": new_id });
    record_event(
        tx.as_mut(),
        "work_item",
        &work_item_id,
        "work_item.research_note_superseded",
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
    use crate::repo::test_support::*;

    /// A superseded research note drops out of the live `get_work_item_detail`
    /// fold; `add_research_note` defaults `state='proposed'` and emits one event.
    #[tokio::test]
    async fn superseded_research_note_excluded_from_live_fold() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let ev_before = count_events(&pool).await;

        let old = add_research_note(&pool, &story, "old finding", None, Some("low"), None, None)
            .await
            .expect("old note")
            .to_string();
        let new = add_research_note(&pool, &story, "new finding", None, Some("high"), None, None)
            .await
            .expect("new note")
            .to_string();
        assert_eq!(count_events(&pool).await, ev_before + 2, "+1 event per add");

        // Both live before supersession; default state is proposed.
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.research_notes.len(), 2, "both notes live");
        assert!(
            detail.research_notes.iter().all(|n| n.state.as_deref() == Some("proposed")),
            "default state proposed"
        );

        // Supersede the old note by the new one.
        supersede_research_note(&pool, &old, &new).await.expect("supersede");

        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.research_notes.len(), 1, "superseded note excluded");
        assert_eq!(detail.research_notes[0].id, new, "only the live note remains");

        // update_research_note set-or-leave: accept the surviving note.
        let req = UpdateResearchNoteRequest {
            confidence: None,
            state: Some(ResearchState::Accepted),
            rationale: Some("chosen".into()),
            lens: None,
        };
        update_research_note(&pool, &new, &req).await.expect("accept");
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.research_notes[0].state.as_deref(), Some("accepted"));
        assert_eq!(detail.research_notes[0].rationale.as_deref(), Some("chosen"));
        assert_eq!(detail.research_notes[0].confidence.as_deref(), Some("high"), "confidence left");
    }
}
