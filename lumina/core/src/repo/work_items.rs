//! Work-item create/update/delete lifecycle (R2 carve).
//!
//! The public CRUD entry points — `create_work_item` and its origin/full
//! variants, the bulk `create_work_items`, the status + generic PATCH updates,
//! and the soft-`delete_work_item` — plus the option/spec structs
//! (`CreateOpts`, `NewWorkItemSpec`). The cross-cluster substrate these compose
//! on (`create_work_item_full_tx`, the closure/epic-done gates, `work_item_kind`,
//! `normalise_object`/`validate_attributes_for_kind`/`validate_plan_field_
//! constraints`, `enum_to_str`) lives in `repo/shared.rs` and is reached via
//! `use super::*`; the event-outbox writers come from `super::events`.
//!
//! `pub use work_items::*` in `repo/mod.rs` PRESERVES the public surface — every
//! `pub` fn here stays reachable at its existing `crate::repo::*` path (the HTTP
//! handlers / MCP tools / importer call them by path and are unchanged). The
//! domain types named in the signatures are imported explicitly from `crate::*`
//! (a `use super::*` glob does NOT carry super's private `use` imports).

use serde_json::Value;
use uuid::Uuid;

use super::*;
// Private item defined in `mod.rs` that this cluster consumes. A child module
// may name its ancestor's private items directly (mirrors `shared.rs`'s explicit
// `use super::{…}` for the same reason).
use super::MAX_BATCH_ITEMS;
use super::events::{record_event, record_inert_event};
use crate::args;
use crate::db::DbClient;
use crate::domain::UpdateWorkItemRequest;
use crate::error::AppError;

/// Create a work item under the single-mutation-path discipline.
///
/// 1. Belt-and-braces hierarchy pre-check (typed 422 on an illegal edge) — runs
///    BEFORE any transaction, so an illegal create writes zero rows.
/// 2. Open one transaction.
/// 3. Insert the `work_items` row (id = freshly-minted UUIDv7 as TEXT).
/// 4. Append ONE `events` row via [`record_event`].
/// 5. Commit. Any error before commit rolls back BOTH writes.
///
/// Returns the new id as a `Uuid`.
///
/// This is the back-compatible 5-arg entry point (no `origin`, default
/// provenance NULL). It delegates to [`create_work_item_with_origin`]; the
/// default `relevance="backlog"` for a new epic/focus/story is applied there.
pub async fn create_work_item(
    pool: &impl DbClient,
    kind: &str,
    parent_id: Option<&str>,
    title: &str,
    body: Option<&str>,
) -> Result<Uuid, AppError> {
    create_work_item_with_origin(pool, kind, parent_id, title, body, None).await
}

/// Create a work item, stamping the optional `origin` provenance (migration
/// 0003). Same single-mutation-path discipline as the 5-arg [`create_work_item`]
/// wrapper. A newly-created `epic`/`focus`/`story` acquires the default
/// `relevance="backlog"` (epic/focus/story carry the relevance axis;
/// task/project are left NULL); the relevance default is applied in the INSERT.
pub async fn create_work_item_with_origin(
    pool: &impl DbClient,
    kind: &str,
    parent_id: Option<&str>,
    title: &str,
    body: Option<&str>,
    origin: Option<&str>,
) -> Result<Uuid, AppError> {
    create_work_item_full(
        pool,
        kind,
        parent_id,
        title,
        body,
        CreateOpts {
            origin,
            outcome: None,
            shape: None,
            lane: None,
        },
    )
    .await
}

/// The create core (migration 0010). The 5-arg [`create_work_item`] and 6-arg
/// [`create_work_item_with_origin`] wrappers delegate here; this 8-arg form adds
/// the `outcome` (epic) and `shape` (focus) channels plus the migration-0010
/// create-time gates. Gates run BEFORE `begin_write` (like the parent pre-check)
/// so an illegal create writes zero rows.
///
/// Create-time gates (User Decisions, ADR lumina epic/focus semantics):
///   * an `epic` requires a non-empty `outcome`;
///   * a `focus` requires a `shape`;
///   * `shape` is valid only on a `focus` (consistency guard);
///   * a `story` may only be created once its ancestor epic has ≥1 close-criterion.
///
/// When `outcome` is supplied (epic only) it is folded into the row's
/// `attributes` JSON (`{"outcome": ...}`) after the same normalise + per-kind
/// validate chain the PATCH path uses. `shape` is bound directly to the
/// `work_items.shape` column. Otherwise identical to the legacy create path:
/// `status="open"`, default `relevance="backlog"` for epic/focus/story, and a
/// single `work_item.created` event.
/// The migration-0010 create-time option channels for [`create_work_item_full`]:
/// `origin` (provenance stamp), `outcome` (epic-only, folds into `attributes`),
/// and `shape` (focus-only, binds the `shape` column). Bundled into a struct so
/// the same-typed `Option<&str>` tail params are passed by name rather than
/// position (R16 — they were previously mis-order-prone).
///
/// `lane` (team-execution): an OPTIONAL caller override for the create-time
/// work-queue lane. The DEFAULT lives in the shared INSERT
/// ([`create_work_item_full_tx`]): when `kind == "task"` and `lane` is `None`, a
/// task is stamped `lane='implement'` so every freshly-planned task is claimable
/// by `claim_next_task` without a separate setter call. A non-task kind ignores
/// `lane` entirely (lane is task-only — stays NULL). Pass `Some(Lane::Review)`
/// (etc.) to override the default for a task.
pub struct CreateOpts<'a> {
    pub origin: Option<&'a str>,
    pub outcome: Option<&'a str>,
    pub shape: Option<&'a str>,
    pub lane: Option<crate::domain::Lane>,
}

pub async fn create_work_item_full(
    db: &impl DbClient,
    kind: &str,
    parent_id: Option<&str>,
    title: &str,
    body: Option<&str>,
    opts: CreateOpts<'_>,
) -> Result<Uuid, AppError> {
    let origin = opts.origin;
    let mut tx = db.begin().await?;
    let id = create_work_item_full_tx(tx.as_mut(), kind, parent_id, title, body, opts).await?;
    let id_str = id.to_string();

    let payload = serde_json::json!({
        "kind": kind,
        "parent_id": parent_id,
        "title": title,
        "origin": origin,
    });
    record_event(tx.as_mut(), "work_item", &id_str, "work_item.created", payload).await?;

    tx.commit().await?;

    Ok(id)
}

/// One work-item spec for the bulk [`create_work_items`] path (B17b). Mirrors the
/// [`create_work_item_full`] arg list (kind/parent/title/body + the [`CreateOpts`]
/// channels origin/outcome/shape) plus the optional spawn provenance
/// `spawned_from_finding_id`.
pub struct NewWorkItemSpec<'a> {
    pub kind: &'a str,
    pub parent_id: Option<&'a str>,
    pub title: &'a str,
    pub body: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub outcome: Option<&'a str>,
    pub shape: Option<&'a str>,
    /// Optional create-time lane override (team-execution). `None` ⇒ the shared
    /// INSERT applies the task-only `'implement'` default (a non-task kind stays
    /// NULL). The bulk-spawn callers that want the default need only leave this
    /// `None`.
    pub lane: Option<crate::domain::Lane>,
    /// When `Some`, stamp `work_items.spawned_from_finding_id` (migration 0011
    /// nullable FK → `findings(id)`) after the INSERT. `create_work_item_full_tx`
    /// deliberately leaves the column NULL on create — this batch spawn path is
    /// the only writer of the column, so the referenced finding must already
    /// exist (FK), or pass `None`.
    pub spawned_from_finding_id: Option<&'a str>,
}

/// Bulk-create a batch of work items under ONE `BEGIN IMMEDIATE` transaction
/// (B17b; plan D8/D10, risk R-B2), all-or-nothing. Each spec is created via
/// [`create_work_item_full_tx`] (which runs ALL create-time validation — the
/// hierarchy edge, the migration-0010 epic-outcome / focus-shape gates, and the
/// story close-criterion gate — and the `work_items` INSERT INSIDE the shared
/// tx), then, when `spawned_from_finding_id` is `Some`, the new row's spawn
/// column is stamped. Returns the new ids in input order.
///
/// ## Parents must already exist (D10)
/// `create_work_item_full_tx`'s parent-kind read runs on the tx, and a missing
/// parent surfaces as [`AppError::Validation`] (`parent work_item '…' does not
/// exist`). This path does NOT support inline `depends_on` nor creating a
/// parent within the same batch — every `parent_id` must reference an EXISTING
/// (committed) work item.
///
/// ## Atomicity (validation aborts the whole batch)
/// Any error from `create_work_item_full_tx` or the spawn-stamp `?`-propagates,
/// dropping `tx` un-committed → SQLite rolls back → ZERO rows persist (a single
/// invalid spec leaves nothing, including the valid specs that preceded it).
///
/// ## Single coarse event (D8 / R-B4)
/// Exactly ONE `events` row is recorded for the whole batch, NOT one per item.
/// Its `aggregate_type` is **deliberately not `"work_item"`**: the git-export
/// drain (`export.rs`) materialises only `aggregate_type="work_item"` events, so
/// a `"work_item"` batch event would wrongly re-render each item N times. A
/// `"batch"`-typed event keyed by a fresh UUIDv7 is correctly inert — drained and
/// `exported_at`-stamped, but not materialised to a file. The accepted
/// consequence (the intended D8/B26 trade-off) is that bulk-created work items are
/// NOT git-exported individually; only the coarse batch event records the write.
pub async fn create_work_items(
    db: &impl DbClient,
    specs: &[NewWorkItemSpec<'_>],
) -> Result<Vec<uuid::Uuid>, AppError> {
    // R14: an empty batch opens no tx and writes no coarse event — return the
    // zero value (an empty id list) up front.
    if specs.is_empty() {
        return Ok(Vec::new());
    }
    // R3: reject an over-cap batch BEFORE any allocation / tx, so an oversized
    // payload cannot balloon allocation or hold the writer lock.
    if specs.len() > MAX_BATCH_ITEMS {
        return Err(AppError::Validation(format!(
            "batch of {} work items exceeds the maximum of {MAX_BATCH_ITEMS} per call",
            specs.len()
        )));
    }

    let mut tx = db.begin().await?;

    let mut ids: Vec<Uuid> = Vec::with_capacity(specs.len());
    for spec in specs {
        // A `create_work_item_full_tx` error `?`-propagates here, dropping `tx`
        // un-committed → full rollback → zero writes (all-or-nothing, D10).
        let id = create_work_item_full_tx(
            tx.as_mut(),
            spec.kind,
            spec.parent_id,
            spec.title,
            spec.body,
            CreateOpts {
                origin: spec.origin,
                outcome: spec.outcome,
                shape: spec.shape,
                lane: spec.lane,
            },
        )
        .await?;

        // B17b owns the spawn stamp: `create_work_item_full_tx` leaves
        // `spawned_from_finding_id` NULL, so set it here when provided. The FK to
        // `findings(id)` is enforced by SQLite — an unknown id aborts the batch.
        if let Some(fid) = spec.spawned_from_finding_id {
            tx.execute(
                "UPDATE work_items SET spawned_from_finding_id = $1 WHERE id = $2",
                args![fid.to_owned(), id.to_string()],
            )
            .await?;
        }

        ids.push(id);
    }

    // Exactly one coarse event for the whole batch (D8). aggregate_type MUST NOT
    // be "work_item" (R-B4) — a fresh UUIDv7 under a "batch" aggregate, which the
    // export drain ignores (there is no run/finding context here, so unlike
    // `add_findings` the only sensible key is a freshly-minted batch id).
    let id_strs: Vec<String> = ids.iter().map(Uuid::to_string).collect();
    let payload = serde_json::json!({ "count": ids.len(), "ids": id_strs });
    record_inert_event(
        tx.as_mut(),
        "batch",
        &Uuid::now_v7().to_string(),
        "work_items.batch_created",
        payload,
    )
    .await?;

    tx.commit().await?;

    Ok(ids)
}

/// Update a work item's free-text status under the single-mutation-path
/// discipline (status update + one event in one transaction). `NotFound` if the
/// id has no row — checked via `rows_affected()` so the missing-row case never
/// emits a spurious event. A `→done` transition on a task is gated by
/// [`enforce_closure_gate`] (the read runs inside the same tx, before the write).
///
/// **Done-is-terminal guard (1B-F9 M4).** A `done → review` transition is
/// REJECTED with [`AppError::Validation`]: `done` is terminal, so a task that
/// already completed (the lite→`done` path, or a reviewer's review→`done` close)
/// can NEVER be flagged back into the review lane. Re-reviewing completed work
/// requires a brand-NEW task (consistent with not_doing #4 / edge-case
/// 019ed5fc-3df0). The row is never reopened.
pub async fn update_work_item_status(
    db: &impl DbClient,
    id: &str,
    status: &str,
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    // Done-is-terminal guard (1B-F9 M4): reject a `done → review` flip BEFORE any
    // write. Read the current status on the tx (same writer-lock snapshot). A
    // missing row reads back None and falls through to the `affected == 0`
    // NotFound below (behaviour preserved). `done` is terminal: the SAME row is
    // never reopened into review — re-review needs a NEW task.
    if status == "review" {
        let current: Option<String> = crate::db::tx_scalar_opt::<String>(
            tx.as_mut(),
            "SELECT status FROM work_items WHERE id = $1 AND deleted_at IS NULL",
            args![id.to_owned()],
        )
        .await?;
        if current.as_deref() == Some("done") {
            return Err(AppError::Validation(format!(
                "work_item '{id}' is done; done is terminal and cannot be flagged into \
                 review — create a new task to re-review completed work"
            )));
        }
    }

    // Closure gate (migration 0003): reject task→done under a `hard` story while
    // any acceptance criterion is unchecked. Runs before the UPDATE in this tx.
    enforce_closure_gate(tx.as_mut(), id, status).await?;
    // Epic-done gate (migration 0010): reject epic→done unless all close-criteria
    // are checked and all descendant stories are terminal. Independent of the
    // closure gate above (task→done vs epic→done); both run.
    enforce_epic_done_gate(tx.as_mut(), id, status).await?;

    let affected = tx
        .execute(
            r#"
        UPDATE work_items
        SET status = $2, updated_at = CURRENT_TIMESTAMP
        WHERE id = $1
        "#,
            args![id.to_owned(), status.to_owned()],
        )
        .await?;

    if affected == 0 {
        // tx drops here → rollback; no event emitted for a missing row.
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "status": status });
    record_event(tx.as_mut(), "work_item", id, "work_item.status_changed", payload).await?;

    tx.commit().await?;

    // T4: reconcile the task's EXPECTED/ACTUAL files_touched sets at close — but
    // ONLY for a `kind='task'` → `done` transition (the non-team close route;
    // `complete_task` is the team-lane route). Not other statuses, and not
    // non-task items (a story/epic/project carries no `task_files` rows). The
    // reconcile COMPOSES after the status commit (it owns its own tx(s) — the
    // Option-A seam) and is IDEMPOTENT, so a re-transition to `done` (or a
    // lease-reclaim re-open→re-close) never re-clears or re-audits. Read the kind
    // only when status is `done` so the common non-`done` transition stays a
    // single round-trip. The kind read uses the autocommit `db` (the status tx is
    // already committed); a missing row would have failed the `affected == 0` gate
    // above, so `work_item_kind` here resolves.
    if status == "done" && work_item_kind(db, id).await? == "task" {
        reconcile_task_files_at_close(db, id).await?;
    }

    Ok(())
}

/// Partial update of a work item under the single-mutation-path discipline.
/// Each field is **set-or-leave**: a `None` bind leaves the column untouched via
/// `COALESCE(?, col)` (it does NOT clear to NULL). If `attributes` is present it
/// is normalised (object-root, null-keys dropped) and per-kind validated
/// (unknown key ⇒ `Validation`) BEFORE the write. `NotFound` via
/// `rows_affected()==0` so a missing row emits no event. Event `work_item.updated`.
pub async fn update_work_item(
    db: &impl DbClient,
    id: &str,
    req: &UpdateWorkItemRequest,
) -> Result<(), AppError> {
    // Pre-validate `attributes` (needs the row's kind) before opening the tx.
    let attributes_str: Option<String> = match &req.attributes {
        Some(value) => {
            let kind = work_item_kind(db, id).await?;
            let cleaned = normalise_object(value, "attributes")?;
            validate_attributes_for_kind(&kind, &cleaned)?;
            validate_plan_field_constraints(&cleaned)?; // R34
            Some(serde_json::to_string(&Value::Object(cleaned)).map_err(|e| AppError::Other(e.into()))?)
        }
        None => None,
    };

    let status_str: Option<String> = req.status.map(enum_to_str);

    let mut tx = db.begin().await?;

    // Closure gate (migration 0003): this generic PATCH can set status="done"
    // directly, so it routes through the SAME gate as update_work_item_status
    // (User Decision 3) — a task→done under a `hard` story with unchecked
    // criteria is rejected here too. No-op when status is absent / not "done".
    if let Some(s) = status_str.as_deref() {
        enforce_closure_gate(tx.as_mut(), id, s).await?;
        // Epic-done gate (migration 0010): same UNCONDITIONAL epic→done rule as
        // the transition_status path; both gates run, they cover disjoint kinds.
        enforce_epic_done_gate(tx.as_mut(), id, s).await?;
    }

    let affected = tx
        .execute(
            r#"
        UPDATE work_items
        SET title      = COALESCE($2, title),
            body       = COALESCE($3, body),
            status     = COALESCE($4, status),
            position   = COALESCE($5, position),
            attributes = COALESCE($6, attributes),
            updated_at = CURRENT_TIMESTAMP
        WHERE id = $1 AND deleted_at IS NULL
        "#,
            args![
                id.to_owned(),
                req.title.clone(),
                req.body.clone(),
                status_str.clone(),
                req.position,
                attributes_str.clone()
            ],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({
        "title": req.title,
        "body": req.body,
        "status": status_str,
        "position": req.position,
        "attributes_set": req.attributes.is_some(),
    });
    record_event(tx.as_mut(), "work_item", id, "work_item.updated", payload).await?;

    tx.commit().await?;
    Ok(())
}

/// SOFT-delete a work item: stamp `deleted_at` under the single-mutation-path
/// discipline. The row (and its cascaded activity) is preserved — a work item
/// owns export identity, so hard-delete would orphan the export TOML and lose
/// history. Idempotent-ish: a row already deleted (or absent) is `NotFound` via
/// `rows_affected()==0`. Event `work_item.deleted`.
pub async fn delete_work_item(db: &impl DbClient, id: &str) -> Result<(), AppError> {
    let mut tx = db.begin().await?;

    // R36: block soft-deleting a `focus` that still has non-terminal, non-deleted
    // child stories. The epic-done gate's rollup counts only stories whose focus
    // parent is `deleted_at IS NULL` (enforce_epic_done_gate), so tombstoning a
    // focus mid-flight would silently drop its live stories from the rollup and
    // let the epic close with non-terminal descendants. Force explicit story
    // disposition first. Read inside the tx through the seam so the liveness check
    // and the soft-delete share one snapshot under the writer lock. A
    // missing/already-deleted id yields kind=None here and falls through to the
    // UPDATE's `affected == 0` NotFound path below — behaviour preserved.
    let kind: Option<String> = crate::db::tx_scalar_opt::<String>(
        tx.as_mut(),
        "SELECT kind FROM work_items WHERE id = $1 AND deleted_at IS NULL",
        args![id.to_owned()],
    )
    .await?;
    if kind.as_deref() == Some("focus") {
        let live_stories: i64 = crate::db::tx_scalar_one::<i64>(
            tx.as_mut(),
            "SELECT COUNT(*) FROM work_items \
             WHERE kind = 'story' AND parent_id = $1 AND deleted_at IS NULL \
             AND status NOT IN ('done','cancelled')",
            args![id.to_owned()],
        )
        .await?;
        if live_stories > 0 {
            return Err(AppError::Validation(format!(
                "focus '{id}' cannot be deleted: {live_stories} non-terminal child \
                 story(ies) remain; resolve or cancel them first"
            )));
        }
    }

    let affected = tx
        .execute(
            r#"UPDATE work_items SET deleted_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NULL"#,
            args![id.to_owned()],
        )
        .await?;

    if affected == 0 {
        return Err(AppError::NotFound(format!("work_item '{id}' not found")));
    }

    let payload = serde_json::json!({ "deleted": true });
    record_event(tx.as_mut(), "work_item", id, "work_item.deleted", payload).await?;

    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::domain::Status;
    use crate::repo::test_support::*;

    /// (a) `create_work_item` inserts exactly one work_items row AND one events
    /// row in one transaction.
    #[tokio::test]
    async fn create_writes_one_work_item_and_one_event() {
        let pool = connect_in_memory().await.expect("pool");

        assert_eq!(count_work_items(&pool).await, 0);
        assert_eq!(count_events(&pool).await, 0);

        let id = create_work_item(&pool, "project", None, "Root", None)
            .await
            .expect("legal project create");

        assert_eq!(count_work_items(&pool).await, 1, "exactly one work_item");
        assert_eq!(count_events(&pool).await, 1, "exactly one event");

        // The event references the new work-item and is unexported (outbox).
        use sqlx::Row as _;
        let ev = sqlx::query(
            r#"SELECT aggregate_id, event_type, exported_at FROM events"#,
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let aggregate_id: String = ev.try_get("aggregate_id").unwrap();
        let event_type: String = ev.try_get("event_type").unwrap();
        let exported_at: Option<String> = ev.try_get("exported_at").unwrap();
        assert_eq!(aggregate_id, id.to_string());
        assert_eq!(event_type, "work_item.created");
        assert!(exported_at.is_none(), "new event must be unexported");
    }

    /// (c) `create_work_item` with an illegal parent kind returns
    /// `AppError::Validation` — NOT a panic, NOT a Db/500.
    #[tokio::test]
    async fn create_with_illegal_parent_kind_is_validation() {
        let pool = connect_in_memory().await.expect("pool");

        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("legal project");

        // task under project is illegal (task's legal parent is story).
        let err = create_work_item(&pool, "task", Some(&project.to_string()), "Bad", None)
            .await
            .expect_err("illegal task→project must error");

        assert!(
            matches!(err, AppError::Validation(_)),
            "expected Validation, got {err:?}"
        );
    }

    /// (b) A failed create rolls back BOTH writes: an illegal create leaves zero
    /// NEW rows. We pre-seed one legal project (1 work_item, 1 event), attempt an
    /// illegal child create, and assert the counts are UNCHANGED.
    #[tokio::test]
    async fn failed_create_leaves_no_new_rows() {
        let pool = connect_in_memory().await.expect("pool");

        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("legal project");
        let wi_before = count_work_items(&pool).await;
        let ev_before = count_events(&pool).await;
        assert_eq!((wi_before, ev_before), (1, 1));

        // Illegal: focus directly under project (focus's legal parent is epic).
        let err = create_work_item(&pool, "focus", Some(&project.to_string()), "Bad", None)
            .await
            .expect_err("illegal create must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        assert_eq!(
            count_work_items(&pool).await,
            wi_before,
            "no new work_item row after a failed create"
        );
        assert_eq!(
            count_events(&pool).await,
            ev_before,
            "no new event row after a failed create"
        );
    }

    /// A legal `task` create under a story succeeds and emits its own event,
    /// proving the full chain plus the per-mutation event invariant across many
    /// writes (5 items ⇒ 5 events).
    #[tokio::test]
    async fn full_chain_then_legal_task() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        // 4 work_items (project/epic/focus/story); 5 events (the four creates plus
        // the epic close-criterion add the migration-0010 story gate requires).
        assert_eq!(count_work_items(&pool).await, 4);
        assert_eq!(count_events(&pool).await, 5);

        let task = create_work_item(&pool, "task", Some(&story), "T", None)
            .await
            .expect("legal task under story");

        assert_eq!(count_work_items(&pool).await, 5);
        assert_eq!(count_events(&pool).await, 6);

        // Detail aggregate: the story has the task as a direct child.
        let detail = get_work_item_detail(&pool, &story).await.expect("detail");
        assert_eq!(detail.item.kind, "story");
        assert_eq!(detail.children.len(), 1);
        assert_eq!(detail.children[0].id, task.to_string());
    }

    // -----------------------------------------------------------------------
    // create_work_items (B17b) — bulk create under one tx, one coarse event,
    // all-or-nothing, with the optional spawn-provenance stamp.
    // -----------------------------------------------------------------------

    /// Bulk-creating N tasks under an existing story inserts exactly N work_items
    /// AND exactly ONE coarse `events` row (D8) — not N events.
    #[tokio::test]
    async fn create_work_items_bulk_under_existing_story() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let wi_before = count_work_items(&pool).await;
        let ev_before = count_events(&pool).await;

        let specs = vec![
            NewWorkItemSpec {
                kind: "task",
                parent_id: Some(&story),
                title: "T1",
                body: None,
                origin: None,
                outcome: None,
                shape: None,
                lane: None,
                spawned_from_finding_id: None,
            },
            NewWorkItemSpec {
                kind: "task",
                parent_id: Some(&story),
                title: "T2",
                body: None,
                origin: None,
                outcome: None,
                shape: None,
                lane: None,
                spawned_from_finding_id: None,
            },
            NewWorkItemSpec {
                kind: "task",
                parent_id: Some(&story),
                title: "T3",
                body: None,
                origin: None,
                outcome: None,
                shape: None,
                lane: None,
                spawned_from_finding_id: None,
            },
        ];
        let n = specs.len() as i64;

        let ids = create_work_items(&pool, &specs)
            .await
            .expect("bulk create under story");
        assert_eq!(ids.len(), specs.len(), "one id returned per spec");

        assert_eq!(
            count_work_items(&pool).await,
            wi_before + n,
            "exactly N new work_items"
        );
        assert_eq!(
            count_events(&pool).await,
            ev_before + 1,
            "exactly ONE coarse batch event for the whole batch (D8)"
        );
        assert_eq!(
            count_events_of_type(&pool, "work_items.batch_created").await,
            1,
            "the coarse event carries the batch event_type"
        );

        // The N tasks exist as direct children of the story.
        let task_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM work_items WHERE parent_id = $1 AND kind = 'task'",
        )
        .bind(&story)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(task_count, n, "all N tasks land under the story");
    }

    /// A spec carrying `spawned_from_finding_id: Some(fid)` stamps the column on
    /// the created task (B17b owns the spawn stamp; the column is NULL on a plain
    /// create). The referenced finding must exist first (FK).
    #[tokio::test]
    async fn create_work_items_stamps_spawned_from_finding_id() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        // The spawn FK targets findings(id), so create a finding on the story first.
        let finding_id = create_finding(&pool, &story, &NewFinding::default())
            .await
            .expect("seed finding")
            .to_string();

        let specs = vec![NewWorkItemSpec {
            kind: "task",
            parent_id: Some(&story),
            title: "spawned task",
            body: None,
            origin: None,
            outcome: None,
            shape: None,
            lane: None,
            spawned_from_finding_id: Some(&finding_id),
        }];

        let ids = create_work_items(&pool, &specs)
            .await
            .expect("create spawned task");
        let task_id = ids[0].to_string();

        let stamped = sqlx::query_scalar::<_, Option<String>>(
            "SELECT spawned_from_finding_id FROM work_items WHERE id = $1",
        )
        .bind(&task_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            stamped,
            Some(finding_id),
            "the spawn column equals the source finding id"
        );
    }

    /// A batch mixing one valid spec with one invalid spec aborts WHOLLY — the
    /// valid spec must NOT persist (all-or-nothing rollback, D10).
    #[tokio::test]
    async fn create_work_items_aborts_whole_batch_on_validation() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let wi_before = count_work_items(&pool).await;
        let ev_before = count_events(&pool).await;

        let specs = vec![
            // Valid: a task under the story.
            NewWorkItemSpec {
                kind: "task",
                parent_id: Some(&story),
                title: "good",
                body: None,
                origin: None,
                outcome: None,
                shape: None,
                lane: None,
                spawned_from_finding_id: None,
            },
            // Invalid: parent_id names no existing work_item → Validation.
            NewWorkItemSpec {
                kind: "task",
                parent_id: Some("no-such-parent"),
                title: "bad",
                body: None,
                origin: None,
                outcome: None,
                shape: None,
                lane: None,
                spawned_from_finding_id: None,
            },
        ];

        let err = create_work_items(&pool, &specs)
            .await
            .expect_err("an invalid spec must abort the batch");
        assert!(
            matches!(err, AppError::Validation(_)),
            "expected Validation, got {err:?}"
        );

        assert_eq!(
            count_work_items(&pool).await,
            wi_before,
            "no work_item persists — the valid spec was rolled back too"
        );
        assert_eq!(
            count_events(&pool).await,
            ev_before,
            "no coarse event on an aborted batch"
        );
    }

    /// `update_work_item_status` updates + emits one event in one tx; a missing
    /// id is `NotFound` and emits NO event.
    #[tokio::test]
    async fn update_status_event_and_notfound() {
        let pool = connect_in_memory().await.expect("pool");
        let id = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        assert_eq!(count_events(&pool).await, 1);

        update_work_item_status(&pool, &id, "in-progress")
            .await
            .expect("status update");
        assert_eq!(count_events(&pool).await, 2, "one new status event");

        let got: String =
            sqlx::query_scalar::<_, String>("SELECT status FROM work_items WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(got, "in-progress");

        // Missing id → NotFound, no event emitted.
        let err = update_work_item_status(&pool, "does-not-exist", "x")
            .await
            .expect_err("missing id must error");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
        assert_eq!(count_events(&pool).await, 2, "no event for a missing-row update");
    }

    /// `update_work_item` writes exactly +1 work_items-row-change and +1 event in
    /// one transaction (set-or-leave: a title update leaves body untouched).
    #[tokio::test]
    async fn update_work_item_writes_one_change_and_one_event() {
        let pool = connect_in_memory().await.expect("pool");
        let id = create_work_item(&pool, "project", None, "Orig", Some("body"))
            .await
            .expect("project")
            .to_string();
        let ev_before = count_events(&pool).await;

        let req = UpdateWorkItemRequest {
            title: Some("Renamed".into()),
            body: None,
            status: Some(Status::InProgress),
            position: None,
            attributes: None,
        };
        update_work_item(&pool, &id, &req).await.expect("update");

        assert_eq!(count_events(&pool).await, ev_before + 1, "+1 event");
        let detail = get_work_item_detail(&pool, &id).await.expect("detail");
        assert_eq!(detail.item.title, "Renamed");
        assert_eq!(detail.item.body.as_deref(), Some("body"), "body left untouched");
        assert_eq!(detail.item.status, "in_progress");
    }

    /// A forced mid-tx error rolls BOTH the work_items change and the event back.
    /// We force the error by giving `update_work_item` an attributes object with
    /// an unknown key for the kind — but that errors pre-tx; to exercise the
    /// rollback we instead drive a known mid-tx failpoint: a unique-violation on
    /// activity seq. Simpler: prove the +0/+0 invariant on the validation reject
    /// path (no tx opened) AND that a NotFound update emits no event.
    #[tokio::test]
    async fn update_work_item_rejects_and_rolls_back() {
        let pool = connect_in_memory().await.expect("pool");
        // story so the per-kind attribute contract applies.
        let story = seed_chain_to_story(&pool).await;
        let wi_events_before = count_events(&pool).await;

        // Unknown attributes key for a story ⇒ Validation, zero new rows/events.
        let bad = UpdateWorkItemRequest {
            title: None,
            body: None,
            status: None,
            position: None,
            attributes: Some(serde_json::json!({ "not_a_story_key": "x" })),
        };
        let err = update_work_item(&pool, &story, &bad)
            .await
            .expect_err("unknown attr key must error");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");
        assert_eq!(
            count_events(&pool).await,
            wi_events_before,
            "no event emitted on a rejected update"
        );

        // NotFound update emits no event either.
        let err = update_work_item(&pool, "missing", &UpdateWorkItemRequest {
            title: Some("x".into()),
            body: None,
            status: None,
            position: None,
            attributes: None,
        })
        .await
        .expect_err("missing id must error");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
        assert_eq!(count_events(&pool).await, wi_events_before, "still no event");
    }

    /// Soft-`delete_work_item` hides the item from `list_work_items` but
    /// `get_work_item_detail` still returns it with `deleted_at` set (well,
    /// returns it at all — the detail SELECT does not filter deleted rows).
    #[tokio::test]
    async fn soft_delete_hides_from_list_but_detail_returns() {
        let pool = connect_in_memory().await.expect("pool");
        let id = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();

        assert_eq!(list_work_items(&pool, None, None).await.unwrap().len(), 1);

        delete_work_item(&pool, &id).await.expect("soft delete");

        assert_eq!(
            list_work_items(&pool, None, None).await.unwrap().len(),
            0,
            "soft-deleted item hidden from list"
        );

        // Detail still resolves the row (does not 404).
        let detail = get_work_item_detail(&pool, &id).await.expect("detail still returns");
        assert_eq!(detail.item.id, id);

        // O17: the detail now surfaces the tombstone instant on the (serde-skipped)
        // `WorkItem.deleted_at` field, so the export fold reads it off the detail.
        assert!(
            detail.item.deleted_at.is_some(),
            "get_work_item_detail surfaces the tombstone on WorkItem.deleted_at"
        );

        // Cross-check the folded value against the raw column.
        let dat: Option<String> =
            sqlx::query_scalar::<_, Option<String>>("SELECT deleted_at FROM work_items WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(dat, detail.item.deleted_at, "detail deleted_at matches the row");

        // Re-deleting is NotFound (already tombstoned).
        let err = delete_work_item(&pool, &id).await.expect_err("re-delete");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
    }

    /// R36: a `focus` with non-terminal child stories cannot be soft-deleted;
    /// once its stories are terminal (done/cancelled) it deletes cleanly. Guards
    /// the epic-done rollup against a tombstoned focus silently dropping live
    /// descendant stories.
    #[tokio::test]
    async fn delete_focus_blocked_while_child_stories_nonterminal() {
        let pool = connect_in_memory().await.expect("pool");
        let project = create_work_item(&pool, "project", None, "P", None)
            .await
            .expect("project");
        let epic = create_work_item_full(
            &pool,
            "epic",
            Some(&project.to_string()),
            "E",
            None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None, lane: None },
        )
        .await
        .expect("epic");
        add_acceptance_criterion(&pool, &epic.to_string(), "c")
            .await
            .expect("criterion");
        let focus = create_work_item_full(
            &pool,
            "focus",
            Some(&epic.to_string()),
            "FO",
            None,
            CreateOpts { origin: None, outcome: None, shape: Some("vertical-slice"), lane: None },
        )
        .await
        .expect("focus");
        let story = create_work_item(&pool, "story", Some(&focus.to_string()), "S", None)
            .await
            .expect("story");

        // Non-terminal (open) story under the focus ⇒ delete blocked.
        let err = delete_work_item(&pool, &focus.to_string())
            .await
            .expect_err("focus delete blocked while a child story is non-terminal");
        assert!(matches!(err, AppError::Validation(_)), "got {err:?}");

        // Make the story terminal, then the focus deletes cleanly.
        update_work_item_status(&pool, &story.to_string(), "done")
            .await
            .expect("story → done");
        delete_work_item(&pool, &focus.to_string())
            .await
            .expect("focus deletes once its stories are terminal");
    }
}
