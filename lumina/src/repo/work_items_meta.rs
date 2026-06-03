//! Work-item meta-mutators (R2 carve): the scalar setters
//! (`set_relevance`/`set_shape`/`set_effort`/`set_complexity`/`set_closure_gate`),
//! the epic/focus plan setters (`set_epic_plan`/`set_focus_plan`), the
//! attributes merge + reorder (`set_work_item_attributes`/`reorder_work_item`),
//! the activity append (`append_activity`), and the context-block link tools
//! (`create_context_block`/`link_context_block`/`unlink_context_block`).
//!
//! The cross-cluster substrate these compose on (`work_item_kind`, `enum_to_str`,
//! `normalise_object`, `validate_attributes_for_kind`,
//! `validate_plan_field_constraints`, `validate_entry_kind`) lives in
//! `repo/shared.rs` and is reached via `use super::*`; the event-outbox writer
//! comes from `super::events`. `pub use work_items_meta::*` in `repo/mod.rs`
//! PRESERVES every `pub` fn's `crate::repo::*` path. Domain types named in the
//! signatures are imported explicitly from `crate::*` (a `use super::*` glob does
//! NOT carry super's private `use` imports).

use serde_json::Value;
use uuid::Uuid;

use super::*;
use super::events::record_event;
use crate::args;
use crate::db::DbClient;
use crate::domain::{ClosureGate, Complexity, Effort, Relevance, Shape};
use crate::error::AppError;

/// Set a work item's `relevance` (migration 0003, User Decision 2). The
/// relevance axis is structural and carried ONLY by epic/focus/story; a
/// `task`/`project` is rejected with a typed [`AppError::Validation`]. The
/// kind is read first; `NotFound` if the id has no row; one event on success.
pub async fn set_relevance(
    db: &impl DbClient,
    id: &str,
    relevance: Relevance,
) -> Result<(), AppError> {
    let kind = work_item_kind(db, id).await?;
    if !matches!(kind.as_str(), "epic" | "focus" | "story") {
        return Err(AppError::Validation(format!(
            "relevance is settable only on epic/focus/story, not on '{kind}'"
        )));
    }
    let value = enum_to_str(relevance);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"UPDATE work_items SET relevance = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
            args![id.to_owned(), value.clone()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "relevance": value });
    record_event(tx.as_mut(), "work_item", id, "work_item.relevance_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Set a focus item's `shape` (migration 0010). Focus-scoped: a non-`focus`
/// kind is rejected with a typed `AppError::Validation`. Kind read first;
/// `NotFound` via rows_affected()==0; one event. This is the revise-later
/// path — shape-mandatory-at-create for focus is enforced in the create path.
pub async fn set_shape(db: &impl DbClient, id: &str, shape: Shape) -> Result<(), AppError> {
    let kind = work_item_kind(db, id).await?;
    if kind != "focus" {
        return Err(AppError::Validation(format!(
            "shape is settable only on a focus, not on '{kind}'"
        )));
    }
    let value = enum_to_str(shape);
    let mut tx = db.begin().await?;
    let affected = tx
        .execute(
            r#"UPDATE work_items SET shape = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
            args![id.to_owned(), value.clone()],
        )
        .await?;
    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }
    let payload = serde_json::json!({ "shape": value });
    record_event(tx.as_mut(), "work_item", id, "work_item.shape_set", payload).await?;
    tx.commit().await?;
    Ok(())
}

/// Revise an epic's plan attributes (migration 0010). Epic-kind-gated; JSON-
/// merges the present fields via set_work_item_attributes (one event). Sibling
/// keys are preserved by the merge. Mandatory-outcome-at-create is enforced in
/// the create path, not here.
pub async fn set_epic_plan(
    pool: &impl DbClient,
    id: &str,
    outcome: Option<&str>,
    context: Option<&str>,
) -> Result<(), AppError> {
    let kind = work_item_kind(pool, id).await?;
    if kind != "epic" {
        return Err(AppError::Validation(format!(
            "epic-plan attributes are settable only on an epic, not on '{kind}'"
        )));
    }
    // R22/R23/R34: the whitespace-only-outcome rejection and the per-field byte
    // cap are now enforced once in `validate_plan_field_constraints`, called from
    // inside `set_work_item_attributes` (the JSON-merge path this delegates to),
    // so they are NOT duplicated here.
    let mut patch = serde_json::Map::new();
    if let Some(v) = outcome {
        patch.insert("outcome".into(), serde_json::Value::String(v.to_string()));
    }
    if let Some(v) = context {
        patch.insert("context".into(), serde_json::Value::String(v.to_string()));
    }
    // no fields supplied — skip the no-op write + spurious event
    if patch.is_empty() {
        return Ok(());
    }
    set_work_item_attributes(pool, id, &serde_json::Value::Object(patch)).await
}

/// Revise a focus's framing (migration 0010). Focus-kind-gated; JSON-merges
/// {framing} via set_work_item_attributes (one event).
pub async fn set_focus_plan(
    pool: &impl DbClient,
    id: &str,
    framing: Option<&str>,
) -> Result<(), AppError> {
    let kind = work_item_kind(pool, id).await?;
    if kind != "focus" {
        return Err(AppError::Validation(format!(
            "focus framing is settable only on a focus, not on '{kind}'"
        )));
    }
    // R23/R34/R42: the per-field byte cap and the whitespace-only-framing
    // rejection are enforced once in `validate_plan_field_constraints`, called
    // from inside `set_work_item_attributes` (the JSON-merge path), so they are
    // NOT duplicated here.
    let mut patch = serde_json::Map::new();
    if let Some(v) = framing {
        patch.insert("framing".into(), serde_json::Value::String(v.to_string()));
    }
    // no fields supplied — skip the no-op write + spurious event
    if patch.is_empty() {
        return Ok(());
    }
    set_work_item_attributes(pool, id, &serde_json::Value::Object(patch)).await
}

/// Set a work item's `effort` grade (migration 0003). Task-scoped: the effort
/// axis drives batch sizing for a leaf task, so a non-`task` kind is rejected
/// with a typed [`AppError::Validation`]. Kind read first; `NotFound` via
/// `rows_affected()==0`; one event.
pub async fn set_effort(db: &impl DbClient, id: &str, effort: Effort) -> Result<(), AppError> {
    let kind = work_item_kind(db, id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "effort is settable only on a task, not on '{kind}'"
        )));
    }
    let value = enum_to_str(effort);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"UPDATE work_items SET effort = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
            args![id.to_owned(), value.clone()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "effort": value });
    record_event(tx.as_mut(), "work_item", id, "work_item.effort_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Set a work item's `complexity` grade (migration 0003). Task-scoped (drives
/// model-tier assignment for a leaf task); a non-`task` kind is rejected with a
/// typed [`AppError::Validation`]. Kind read first; `NotFound` via
/// `rows_affected()==0`; one event.
pub async fn set_complexity(
    db: &impl DbClient,
    id: &str,
    complexity: Complexity,
) -> Result<(), AppError> {
    let kind = work_item_kind(db, id).await?;
    if kind != "task" {
        return Err(AppError::Validation(format!(
            "complexity is settable only on a task, not on '{kind}'"
        )));
    }
    let value = enum_to_str(complexity);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"UPDATE work_items SET complexity = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
            args![id.to_owned(), value.clone()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "complexity": value });
    record_event(tx.as_mut(), "work_item", id, "work_item.complexity_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Set a story's `closure_gate` (migration 0003, User Decision 3). Story-scoped:
/// the gate decides whether tasks under the story reject a `→done` transition
/// while their acceptance criteria are unchecked (`hard`) or merely flag it
/// (`soft`). A non-`story` kind is rejected with a typed [`AppError::Validation`].
/// Kind read first; `NotFound` via `rows_affected()==0`; one event.
pub async fn set_closure_gate(
    db: &impl DbClient,
    story_id: &str,
    gate: ClosureGate,
) -> Result<(), AppError> {
    let kind = work_item_kind(db, story_id).await?;
    if kind != "story" {
        return Err(AppError::Validation(format!(
            "closure_gate is settable only on a story, not on '{kind}'"
        )));
    }
    let value = enum_to_str(gate);

    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"UPDATE work_items SET closure_gate = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
            args![story_id.to_owned(), value.clone()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{story_id}' not found")));
    }

    let payload = serde_json::json!({ "closure_gate": value });
    record_event(tx.as_mut(), "work_item", story_id, "work_item.closure_gate_set", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Append ONE `work_item_activity` row under the single-mutation-path discipline.
/// `seq` is allocated as `MAX(seq)+1` for the item WITHIN the transaction; the
/// `UNIQUE(work_item_id, seq)` constraint makes a race surface as a constraint
/// violation rather than silent duplication. `entry_kind` is validated against
/// the [`ActivityType`] set (typed `Validation`, not panic); `payload`, if
/// present, is normalised (object-root, null-keys dropped). The work item must
/// exist (`NotFound` otherwise). Event `work_item.activity_appended`. Returns the
/// new activity row id.
pub async fn append_activity(
    db: &impl DbClient,
    work_item_id: &str,
    entry_kind: &str,
    author: Option<&str>,
    summary: &str,
    payload: Option<&Value>,
    origin: Option<&str>,
) -> Result<Uuid, AppError> {
    let entry_kind = validate_entry_kind(entry_kind)?;

    let payload_str: Option<String> = match payload {
        Some(value) => {
            let cleaned = normalise_object(value, "activity payload")?;
            Some(
                serde_json::to_string(&Value::Object(cleaned))
                    .map_err(|e| AppError::Other(e.into()))?,
            )
        }
        None => None,
    };

    // Verify the work item exists first (NotFound, not a dangling-FK 500).
    let _ = work_item_kind(db, work_item_id).await?;

    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    // Allocate the per-item monotonic seq inside the tx.
    let seq = crate::db::tx_scalar_one::<i64>(
        tx.as_mut(),
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM work_item_activity WHERE work_item_id = $1",
        args![work_item_id.to_owned()],
    )
    .await?;

    tx.execute(
        "INSERT INTO work_item_activity (id, work_item_id, seq, entry_kind, author, summary, payload, origin) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        args![
            id_str.clone(),
            work_item_id.to_owned(),
            seq,
            entry_kind.to_owned(),
            author.map(str::to_owned),
            summary.to_owned(),
            payload_str,
            origin.map(str::to_owned)
        ],
    )
    .await?;

    let event_payload = serde_json::json!({
        "activity_id": id_str,
        "seq": seq,
        "entry_kind": entry_kind,
    });
    record_event(tx.as_mut(), "work_item", work_item_id, "work_item.activity_appended", event_payload)
        .await?;

    tx.commit().await?;
    Ok(id)
}

/// Read-modify-merge a work item's `attributes`: SELECT the current object,
/// overwrite the keys present in `patch`, leave absent keys, normalise
/// (object-root, drop null-valued keys), per-kind validate, write back. This is
/// the fn the MCP `set_story_plan`/`set_task_spec` partial setters compose on, so
/// merging must NOT clobber sibling keys. One event `work_item.updated`.
///
/// # Why Rust-side merge (not SQL `json_patch`) — T3
///
/// We deliberately retain the Rust-side read+merge inside the transaction
/// rather than swapping in `UPDATE … SET attributes = json_patch(attributes, ?)`.
/// The validator chain ([`normalise_object`] + [`validate_attributes_for_kind`])
/// must run on the MERGED MAP, not on the patch alone, so that an unknown key
/// (which the per-kind validator rejects) and a non-object root (rejected by
/// `normalise_object`) are surfaced as a clean typed [`AppError::Validation`]
/// (→ 422) instead of a constraint-free `json_patch` overwrite. The atomicity
/// gain — read and write are committed together, or neither — comes from the
/// surrounding [`crate::db::begin_write`] tx, not from the SQL primitive.
///
/// # Null-key semantics — unsupported via this entry point (T3)
///
/// `normalise_object` strips null-valued keys on every call, so a patch shaped
/// `{"x": null}` does NOT delete an existing `x` key — it is silently dropped
/// from the patch before the merge. This is intentional: the widened
/// `set_story_plan` callers (the `not-doing` and `verification-commands`
/// SKILLs) never pass null values, only omitted-or-string. Callers needing
/// explicit key-deletion semantics must go through a future dedicated
/// `clear_attribute_key` path; do not work around it by storing an empty
/// string or by editing `normalise_object` to preserve nulls (the TOML export
/// path depends on null-key stripping).
pub async fn set_work_item_attributes(
    db: &impl DbClient,
    id: &str,
    patch: &Value,
) -> Result<(), AppError> {
    // The patch itself must be a null-free object root.
    let patch_obj = normalise_object(patch, "attributes")?;

    let mut tx = db.begin().await?;

    // Read current kind + attributes (do not resurrect a tombstoned row).
    let (current_kind, current_attributes) =
        crate::db::tx_query_opt::<(String, Option<String>)>(
            tx.as_mut(),
            r#"SELECT kind, attributes FROM work_items WHERE id = $1 AND deleted_at IS NULL"#,
            args![id.to_owned()],
        )
        .await?
        .ok_or_else(|| AppError::NotFound(format!("work_item '{id}' not found")))?;

    // Merge: start from the existing object (or empty), overwrite present keys.
    // A stored blob that is non-JSON or a non-object root is data corruption (the
    // write side normalises every stored value to an object root) — fail loudly
    // as `Other` (→ 500) rather than silently discarding it (R13), mirroring
    // `decode_attributes`.
    let mut merged: serde_json::Map<String, Value> = match current_attributes {
        Some(s) => match serde_json::from_str::<Value>(&s) {
            Ok(Value::Object(m)) => m,
            Ok(_) => {
                return Err(AppError::Other(anyhow::anyhow!(
                    "stored attributes for work_item '{id}' is not a JSON object (corrupt blob)"
                )));
            }
            Err(e) => return Err(AppError::Other(e.into())),
        },
        None => serde_json::Map::new(),
    };
    for (k, v) in patch_obj {
        merged.insert(k, v);
    }
    // Re-normalise the merged result (drop any nulls a prior store missed).
    let merged_value = Value::Object(merged);
    let cleaned = normalise_object(&merged_value, "attributes")?;
    validate_attributes_for_kind(&current_kind, &cleaned)?;
    validate_plan_field_constraints(&cleaned)?; // R34

    let merged_str =
        serde_json::to_string(&Value::Object(cleaned)).map_err(|e| AppError::Other(e.into()))?;

    tx.execute(
        r#"UPDATE work_items SET attributes = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NULL"#,
        args![id.to_owned(), merged_str],
    )
    .await?;

    let payload = serde_json::json!({ "attributes_merged": true });
    record_event(tx.as_mut(), "work_item", id, "work_item.updated", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Set a work item's sibling-ordering `position` under the single-mutation-path
/// discipline. Reuses the `work_item.updated` event type (matches the
/// `update_work_item` partial-update convention — position is one of its
/// COALESCE fields). `NotFound` via `rows_affected()==0`.
pub async fn reorder_work_item(
    db: &impl DbClient,
    id: &str,
    position: i64,
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            r#"UPDATE work_items SET position = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NULL"#,
            args![id.to_owned(), position],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "position": position });
    record_event(tx.as_mut(), "work_item", id, "work_item.updated", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Create a `context_blocks` row under the single-mutation-path discipline.
/// Returns the new id. Event `context_block.created`.
pub async fn create_context_block(
    db: &impl DbClient,
    title: Option<&str>,
    body: Option<&str>,
) -> Result<Uuid, AppError> {
    let id = Uuid::now_v7();
    let id_str = id.to_string();

    let mut tx = db.begin().await?;

    tx.execute(
        "INSERT INTO context_blocks (id, title, body) VALUES ($1, $2, $3)",
        args![id_str.clone(), title.map(str::to_owned), body.map(str::to_owned)],
    )
    .await?;

    let payload = serde_json::json!({ "title": title });
    record_event(tx.as_mut(), "context_block", &id_str, "context_block.created", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Link a context block to a work item (insert the `work_item_context` row)
/// under the single-mutation-path discipline. Event `context_block.linked`.
pub async fn link_context_block(
    db: &impl DbClient,
    work_item_id: &str,
    context_block_id: &str,
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    tx.execute(
        "INSERT INTO work_item_context (work_item_id, context_block_id) VALUES ($1, $2)",
        args![work_item_id.to_owned(), context_block_id.to_owned()],
    )
    .await?;

    let payload = serde_json::json!({ "context_block_id": context_block_id });
    record_event(tx.as_mut(), "work_item", work_item_id, "context_block.linked", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// Unlink a context block from a work item (hard-delete the link row — links
/// have no independent export identity) under the single-mutation-path
/// discipline. `NotFound` via `rows_affected()==0`. Event `context_block.unlinked`.
pub async fn unlink_context_block(
    db: &impl DbClient,
    work_item_id: &str,
    context_block_id: &str,
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    let affected = tx
        .execute(
            "DELETE FROM work_item_context WHERE work_item_id = $1 AND context_block_id = $2",
            args![work_item_id.to_owned(), context_block_id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!(
            "context link (work_item '{work_item_id}', block '{context_block_id}') not found"
        )));
    }

    let payload = serde_json::json!({ "context_block_id": context_block_id });
    record_event(tx.as_mut(), "work_item", work_item_id, "context_block.unlinked", payload).await?;

    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::repo::test_support::*;

    /// `append_activity` writes one activity row with monotonic per-item `seq`
    /// and one event each; payload is normalised; an unknown entry_kind is
    /// `Validation`.
    #[tokio::test]
    async fn append_activity_monotonic_seq_and_event() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let ev_before = count_events(&pool).await;

        append_activity(&pool, &story, "execution", Some("alice"), "did a thing", None, None)
            .await
            .expect("first activity");
        append_activity(
            &pool,
            &story,
            "comment",
            None,
            "second",
            Some(&serde_json::json!({ "k": "v", "drop_me": null })),
            Some("implement"),
        )
        .await
        .expect("second activity");

        assert_eq!(count_activity(&pool).await, 2);
        assert_eq!(count_events(&pool).await, ev_before + 2, "+1 event per append");

        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.activity.len(), 2);
        assert_eq!(detail.activity[0].seq, 1);
        assert_eq!(detail.activity[1].seq, 2, "seq is monotonic per item");
        // origin stamps round-trip: first entry omitted it (NULL), second set it.
        assert_eq!(detail.activity[0].origin, None, "no origin ⇒ NULL");
        assert_eq!(
            detail.activity[1].origin.as_deref(),
            Some("implement"),
            "origin stamp persisted and round-tripped"
        );
        // null-valued payload key was dropped on normalise.
        let payload = detail.activity[1].payload.as_ref().expect("payload");
        assert!(payload.get("k").is_some());
        assert!(payload.get("drop_me").is_none(), "null key dropped");

        // Unknown entry_kind ⇒ Validation.
        let err = append_activity(&pool, &story, "nonsense", None, "x", None, None)
            .await
            .expect_err("unknown entry_kind must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        assert_eq!(count_activity(&pool).await, 2, "no row for a rejected append");
    }

    /// A `set_story_plan`-style partial merge: calling `set_work_item_attributes`
    /// twice with DIFFERENT keys leaves the earlier sibling key intact.
    #[tokio::test]
    async fn set_work_item_attributes_merges_without_clobber() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        set_work_item_attributes(&pool, &story, &serde_json::json!({ "problem_statement": "P" }))
            .await
            .expect("first merge");
        set_work_item_attributes(&pool, &story, &serde_json::json!({ "research_notes": "R" }))
            .await
            .expect("second merge");

        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        let attrs = detail.item.attributes.expect("attributes set");
        assert_eq!(attrs.get("problem_statement").and_then(|v| v.as_str()), Some("P"), "sibling intact");
        assert_eq!(attrs.get("research_notes").and_then(|v| v.as_str()), Some("R"));
    }

    /// An attributes object with an unknown key for a kind returns `Validation`
    /// (NOT a 500/panic), and a non-object root is also `Validation`.
    #[tokio::test]
    async fn attributes_validation_is_typed() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let err = set_work_item_attributes(&pool, &story, &serde_json::json!({ "bogus": 1 }))
            .await
            .expect_err("unknown key");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        let err = set_work_item_attributes(&pool, &story, &serde_json::json!([1, 2, 3]))
            .await
            .expect_err("array root");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }

    /// `set_relevance` is rejected on a task (typed Validation) and accepted on a
    /// story. Also asserts a freshly-created story defaults to `relevance="backlog"`.
    #[tokio::test]
    async fn set_relevance_scope_and_default_backlog() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        // Default relevance on a created story is "backlog".
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.item.relevance.as_deref(), Some("backlog"), "story defaults backlog");

        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();
        // task has NULL relevance on create.
        let tdetail = get_work_item_detail(&pool, &task).await.expect("task detail");
        assert!(tdetail.item.relevance.is_none(), "task relevance NULL on create");

        // set_relevance on a task → Validation.
        let err = set_relevance(&pool, &task, Relevance::Active)
            .await
            .expect_err("relevance on task must reject");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // set_relevance on a story → ok.
        set_relevance(&pool, &story, Relevance::Active).await.expect("story relevance ok");
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.item.relevance.as_deref(), Some("active"));
    }

    /// `set_effort`/`set_complexity` are task-scoped (reject a story);
    /// `set_closure_gate` is story-scoped (reject a task).
    #[tokio::test]
    async fn effort_complexity_closure_gate_scopes() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();

        set_effort(&pool, &task, Effort::M).await.expect("effort on task ok");
        set_complexity(&pool, &task, Complexity::High).await.expect("complexity on task ok");
        let detail = get_work_item_detail(&pool, &task).await.expect("detail");
        assert_eq!(detail.item.effort.as_deref(), Some("m"));
        assert_eq!(detail.item.complexity.as_deref(), Some("high"));

        let err = set_effort(&pool, &story, Effort::S).await.expect_err("effort on story rejects");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        let err = set_complexity(&pool, &story, Complexity::Low)
            .await
            .expect_err("complexity on story rejects");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        set_closure_gate(&pool, &story, ClosureGate::Soft).await.expect("gate on story ok");
        let err = set_closure_gate(&pool, &task, ClosureGate::Hard)
            .await
            .expect_err("gate on task rejects");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
    }
}
