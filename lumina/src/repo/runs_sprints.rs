//! Runs / sprints / triage-decisions (R3 carve, migration 0011) — the
//! review/optimise findings-queue domain (B23). Every mutator follows the
//! single-mutation-path discipline (one `db::begin` tx, the domain write(s),
//! EXACTLY ONE `record_event`, one commit).
//!
//! **Export-inert routing (R-B4).** `runs`, `sprints`, and `finding_decisions`
//! are NOT git-exported entities — the export drain (`export.rs`) materialises
//! ONLY `aggregate_type = "work_item"` events. So every event in this section is
//! routed to a NON-`"work_item"` aggregate (`"run"` / `"sprint"` / `"finding"`),
//! mirroring how `add_findings` / `batch_update_findings` / `create_finding`
//! pick inert aggregates: the event drains and is `exported_at`-stamped but
//! renders no file. A spawn decision (which DOES create a `work_item`) routes the
//! CHILD's `work_item.created` event through `create_work_item_full_tx`'s caller
//! — but B23 deliberately uses `create_work_item_full_tx` (the no-event tx
//! helper), folding the spawn into the decision's single `"finding"` event so the
//! whole decision is one event, NOT two. (Resolve is the documented exception —
//! see `record_finding_decision`.)
//!
//! **R2 — the sharper consequence of inert routing.** Because a spawned work-item's
//! `work_item.created` is folded into the inert `"finding"` event (and bulk-created
//! items into a `"batch"` event), the spawned/bulk-created rows get NO git-export
//! snapshot at creation time — the export drain only renders `work_item` events.
//! The audit trail on disk is therefore SILENTLY INCOMPLETE for these items until a
//! LATER mutation touches one (emitting its own `work_item.*` event, which the
//! drain then materialises). This is the accepted D8/R-B4 trade-off, not a bug, but
//! it means "no exported TOML yet" is the expected steady state for a freshly
//! spawned item, not a sign of a dropped event.
//!
//! `pub use runs_sprints::*` in `repo/mod.rs` PRESERVES the public surface — every
//! `pub` fn here stays reachable at its existing `crate::repo::*` path (the HTTP
//! handlers / MCP tools / importer call them by path and are unchanged). The
//! domain types named in the signatures are imported explicitly from `crate::*`
//! (a `use super::*` glob does NOT carry super's private `use` imports). The
//! cross-cluster substrate (`create_work_item_full_tx`, `CreateOpts`, `enum_to_str`)
//! lives in `repo/shared.rs` and is reached via `use super::*`.

use uuid::Uuid;

use super::*;
use super::events::{record_event, record_inert_event};
use crate::args;
use crate::db::{DbClient, Scalar};
use crate::domain::{
    Disposition, FindingDecisionKind, NewFindingDecision, NewRun, NewSprint, SprintStatus,
    TargetKind, TriageState,
};
use crate::error::AppError;

/// Open a new review/optimise [`run`](crate::domain::NewRun) over a live story
/// or an existing sprint (migration 0011, B23). The target is validated BEFORE
/// the transaction opens so an absent / wrong-kind / tombstoned target is a
/// clean [`AppError::Validation`] (→ 422) rather than a dangling-FK 500:
///   * `TargetKind::Story` requires a LIVE `kind='story'` row (`deleted_at IS
///     NULL`) — a tombstoned story is rejected;
///   * `TargetKind::Sprint` requires a `sprints` row.
///
/// Single-mutation-path: one `runs` INSERT (`status` left to the column DEFAULT
/// `'open'`, omitted from the column list — mirroring how `create_finding_tx`
/// omits `triage_state`) + EXACTLY ONE export-inert `run.created` event
/// (`aggregate_type="run"`; R-B4 — never `"work_item"`). Returns the run id.
pub async fn create_run(db: &impl DbClient, run: &NewRun) -> Result<Uuid, AppError> {
    // Validate the target exists, is live, and matches `target_kind` BEFORE the
    // tx — a clean Validation, never a 500.
    match run.target_kind {
        TargetKind::Story => {
            let live = db
                .query_opt::<Scalar<i64>>(
                    "SELECT 1 FROM work_items \
                     WHERE id = $1 AND kind = 'story' AND deleted_at IS NULL",
                    args![run.target_id.clone()],
                )
                .await?
                .is_some();
            if !live {
                return Err(AppError::Validation(format!(
                    "run target '{}' is not a live story",
                    run.target_id
                )));
            }
        }
        TargetKind::Sprint => {
            let exists = db
                .query_opt::<Scalar<i64>>(
                    "SELECT 1 FROM sprints WHERE id = $1",
                    args![run.target_id.clone()],
                )
                .await?
                .is_some();
            if !exists {
                return Err(AppError::Validation(format!(
                    "run target '{}' is not an existing sprint",
                    run.target_id
                )));
            }
        }
    }

    let id = Uuid::now_v7();
    let id_str = id.to_string();
    let kind_str = enum_to_str(run.kind);
    let target_kind_str = enum_to_str(run.target_kind);

    let mut tx = db.begin().await?;

    // `status` is omitted so the column DEFAULT ('open') applies.
    tx.execute(
        "INSERT INTO runs (id, kind, target_id, target_kind) VALUES ($1, $2, $3, $4)",
        args![
            id_str.clone(),
            kind_str.clone(),
            run.target_id.clone(),
            target_kind_str.clone()
        ],
    )
    .await?;

    // One export-inert event (R-B4): aggregate_type="run", NOT "work_item".
    let payload = serde_json::json!({
        "kind": kind_str,
        "target_id": run.target_id,
        "target_kind": target_kind_str,
    });
    record_inert_event(tx.as_mut(), "run", &id_str, "run.created", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Create a new (previously-ephemeral) [`sprint`](crate::domain::NewSprint)
/// grouping (migration 0011, B23; widened migration 0016). Single-mutation-path:
/// one `sprints` INSERT (`title` nullable from the input; `status` written
/// EXPLICITLY as `'draft'` — the pre-0016 column DEFAULT `'open'` is now
/// vestigial — plus the optional `worktree_id` / `predecessor_sprint_id`
/// run-chaining columns) + EXACTLY ONE export-inert `sprint.created` event
/// (`aggregate_type="sprint"`; R-B4 — never `"work_item"`). Returns the sprint
/// id.
///
/// The two new FK columns are VALIDATED to reference existing rows BEFORE the tx
/// opens so a dangling reference is a clean [`AppError::Validation`] (→ 422)
/// rather than a dangling-FK 500 (mirroring [`create_run`]'s target check):
/// a `Some(worktree_id)` must name a `worktrees` row; a
/// `Some(predecessor_sprint_id)` must name a `sprints` row.
pub async fn create_sprint(db: &impl DbClient, sprint: &NewSprint) -> Result<Uuid, AppError> {
    // Validate the optional FK references BEFORE the tx — a clean Validation,
    // never a dangling-FK 500.
    if let Some(worktree_id) = sprint.worktree_id.as_deref() {
        let exists = db
            .query_opt::<Scalar<i64>>(
                "SELECT 1 FROM worktrees WHERE id = $1",
                args![worktree_id.to_owned()],
            )
            .await?
            .is_some();
        if !exists {
            return Err(AppError::Validation(format!(
                "sprint worktree_id '{worktree_id}' does not name an existing worktree"
            )));
        }
    }
    if let Some(predecessor_id) = sprint.predecessor_sprint_id.as_deref() {
        let exists = db
            .query_opt::<Scalar<i64>>(
                "SELECT 1 FROM sprints WHERE id = $1",
                args![predecessor_id.to_owned()],
            )
            .await?
            .is_some();
        if !exists {
            return Err(AppError::Validation(format!(
                "sprint predecessor_sprint_id '{predecessor_id}' does not name an existing sprint"
            )));
        }
    }

    let id = Uuid::now_v7();
    let id_str = id.to_string();
    // `status` is written EXPLICITLY as the migration-0016 create-default
    // `'draft'` (the column DEFAULT 'open' is now vestigial).
    let status_str = enum_to_str(SprintStatus::Draft);

    let mut tx = db.begin().await?;

    tx.execute(
        "INSERT INTO sprints (id, title, status, worktree_id, predecessor_sprint_id) \
         VALUES ($1, $2, $3, $4, $5)",
        args![
            id_str.clone(),
            sprint.title.clone(),
            status_str.clone(),
            sprint.worktree_id.clone(),
            sprint.predecessor_sprint_id.clone()
        ],
    )
    .await?;

    let payload = serde_json::json!({
        "title": sprint.title,
        "worktree_id": sprint.worktree_id,
        "predecessor_sprint_id": sprint.predecessor_sprint_id,
    });
    record_inert_event(tx.as_mut(), "sprint", &id_str, "sprint.created", payload).await?;

    tx.commit().await?;
    Ok(id)
}

/// Transition a sprint's lifecycle status (migration 0016), enforcing the
/// [`SprintStatus`] legal-transition table at the REPO layer (the
/// `sprints.status` column is free TEXT with NO DB CHECK). Single-mutation-path:
/// one `sprints` status UPDATE + EXACTLY ONE export-inert `sprint.status_changed`
/// event (`aggregate_type="sprint"`; R-B4 — never `"work_item"`).
///
/// Gating, in order:
///   1. The sprint must exist and be live → else [`AppError::NotFound`].
///   2. The stored status string must parse to a [`SprintStatus`]; a legacy /
///      out-of-vocab string is a clean [`AppError::Validation`] (NEVER a parse
///      panic).
///   3. `current.can_transition_to(next)` must hold → else
///      [`AppError::Validation`] naming `current → next`.
///   4. **Worktree-owner terminal guard.** A terminal transition out of `review`
///      (`review → done` or `review → cancelled`) is REJECTED when the sprint
///      OWNS a worktree (`EXISTS worktrees WHERE owning_sprint_id = sprint`):
///      the worktree's merge AUDIT must be recorded through
///      `record_worktree_merge` / `record_worktree_rejection`, never skipped via
///      a bare status flip. A worktree-LESS sprint transitions normally.
pub async fn set_sprint_status(
    db: &impl DbClient,
    sprint_id: &str,
    next: SprintStatus,
) -> Result<(), AppError> {
    // 1. Read the current status → NotFound if absent. (The `sprints` table has
    //    no soft-delete column — rows are never tombstoned — so there is no
    //    `deleted_at IS NULL` filter, unlike the `work_items` reads.)
    let current_str: String = match crate::db::scalar_opt::<String>(
        db,
        "SELECT status FROM sprints WHERE id = $1",
        args![sprint_id.to_owned()],
    )
    .await?
    {
        Some(s) => s,
        None => return Err(AppError::NotFound(format!("sprint '{sprint_id}' not found"))),
    };

    // 2. Parse the stored string into the typed enum — an unrecognised / legacy
    //    value is a clean Validation, never a parse panic/unwrap.
    let current: SprintStatus =
        serde_json::from_value(serde_json::Value::String(current_str.clone())).map_err(|_| {
            AppError::Validation(format!(
                "sprint '{sprint_id}' has unrecognised status '{current_str}'"
            ))
        })?;

    let next_str = enum_to_str(next);

    // 3. Legal-transition gate.
    if !current.can_transition_to(next) {
        return Err(AppError::Validation(format!(
            "illegal sprint status transition '{current_str}' → '{next_str}'"
        )));
    }

    // 4. Worktree-owner terminal guard: a `review → done|cancelled` transition on
    //    a worktree-OWNING sprint must go through the merge/rejection audit path.
    if matches!(current, SprintStatus::Review)
        && matches!(next, SprintStatus::Done | SprintStatus::Cancelled)
    {
        let owns_worktree = db
            .query_opt::<Scalar<i64>>(
                "SELECT 1 FROM worktrees WHERE owning_sprint_id = $1",
                args![sprint_id.to_owned()],
            )
            .await?
            .is_some();
        if owns_worktree {
            return Err(AppError::Validation(format!(
                "sprint '{sprint_id}' owns a worktree; use record_worktree_merge / \
                 record_worktree_rejection to take it terminal, not set_sprint_status"
            )));
        }
    }

    // 5. One BEGIN IMMEDIATE tx: the status UPDATE + EXACTLY ONE export-inert
    //    `sprint.status_changed` event.
    let mut tx = db.begin().await?;

    // NOTE: `sprints` has no `updated_at` column (only `created_at`), so the
    // status flip updates `status` alone — adding `updated_at = CURRENT_TIMESTAMP`
    // would reference a non-existent column and fail at runtime.
    tx.execute(
        "UPDATE sprints SET status = $2 WHERE id = $1",
        args![sprint_id.to_owned(), next_str.clone()],
    )
    .await?;

    let payload = serde_json::json!({ "from": current_str, "to": next_str });
    record_inert_event(
        tx.as_mut(),
        "sprint",
        sprint_id,
        "sprint.status_changed",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Add one or more tasks to a sprint via the `sprint_tasks` junction (migration
/// 0011, B23), all-or-nothing. The sprint is validated BEFORE the loop; then,
/// inside ONE tx, every `task_id` is validated as a LIVE `kind='task'` row — a
/// missing / non-task id aborts the WHOLE batch (mirroring
/// [`batch_update_findings`]), so a partial membership never persists. Each
/// membership is an `INSERT … ON CONFLICT(sprint_id, task_id) DO NOTHING`, so
/// re-adding an already-member task is a no-op (`rows_affected()==0`), NOT an
/// error — only genuinely-new memberships count toward the returned `added`.
///
/// Single-mutation-path: the N junction INSERTs + EXACTLY ONE export-inert
/// coarse `sprint.tasks_added` event (`aggregate_type="sprint"`, keyed by the
/// sprint id; R-B4 — never `"work_item"`), payload `{added, requested}`.
/// Returns the count of memberships actually inserted.
pub async fn add_tasks_to_sprint(
    db: &impl DbClient,
    sprint_id: &str,
    task_ids: &[&str],
) -> Result<u64, AppError> {
    // Validate the sprint exists BEFORE the loop (NotFound, not a dangling-FK 500).
    let sprint_exists = db
        .query_opt::<Scalar<i64>>(
            "SELECT 1 FROM sprints WHERE id = $1",
            args![sprint_id.to_owned()],
        )
        .await?
        .is_some();
    if !sprint_exists {
        return Err(AppError::NotFound(format!("sprint '{sprint_id}' not found")));
    }

    let mut tx = db.begin().await?;

    let mut added: u64 = 0;
    for &task_id in task_ids {
        // Validate the id is a LIVE task — a non-task / missing id aborts the
        // whole batch (`?`-propagated rollback → zero memberships persist).
        let kind: Option<String> = crate::db::tx_scalar_opt::<String>(
            tx.as_mut(),
            "SELECT kind FROM work_items WHERE id = $1 AND deleted_at IS NULL",
            args![task_id.to_owned()],
        )
        .await?;
        match kind.as_deref() {
            Some("task") => {}
            _ => {
                return Err(AppError::Validation(format!(
                    "sprint member '{task_id}' is not a live task"
                )));
            }
        }

        let affected = tx
            .execute(
                "INSERT INTO sprint_tasks (sprint_id, task_id) VALUES ($1, $2) \
                 ON CONFLICT(sprint_id, task_id) DO NOTHING",
                args![sprint_id.to_owned(), task_id.to_owned()],
            )
            .await?;
        // `affected == 0` ⇒ a dedup skip (already a member), NOT an error.
        if affected == 1 {
            added += 1;
        }
    }

    // One export-inert coarse event (R-B4): aggregate_type="sprint", keyed by the
    // sprint id, NOT "work_item".
    let payload = serde_json::json!({ "added": added, "requested": task_ids.len() });
    record_inert_event(
        tx.as_mut(),
        "sprint",
        sprint_id,
        "sprint.tasks_added",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok(added)
}

/// The sprint a task is bound to via `sprint_tasks`, most-recent attachment
/// winning, or `None` when the task is not a sprint member. Read-only, no
/// transaction.
///
/// Moves the former inline `get_session_context` MCP-layer scalar probe behind
/// the repo seam (review R11), keeping that tool SQL-free. `sprint_tasks` has no
/// recency column (its PK is `(sprint_id, task_id)`), so `ORDER BY rowid DESC`
/// makes a re-sprinted task bind deterministically to its NEWEST attachment —
/// SQLite's `rowid` is insertion-monotonic (review R12).
pub async fn sprint_for_task(
    db: &impl DbClient,
    task_id: &str,
) -> Result<Option<String>, AppError> {
    crate::db::scalar_opt::<String>(
        db,
        "SELECT sprint_id FROM sprint_tasks WHERE task_id = $1 ORDER BY rowid DESC LIMIT 1",
        args![task_id.to_owned()],
    )
    .await
}

/// Record a triage [`decision`](crate::domain::NewFindingDecision) against a
/// finding (migration 0011, B23), returning `(decision_id,
/// spawned_work_item_id)` — the second element is `Some` only for the two spawn
/// verdicts.
///
/// ## Decision → behaviour map (the B23 judgement core)
/// The plan leaves the per-decision `triage_state`, the spawn parent, the title
/// source, and the `Resolve` disposition UNDER-SPECIFIED; this implements the
/// orchestrator's chosen, internally-consistent design:
///   * `SpawnTask` → create a child `task` under the finding's host work_item;
///     `triage_state = "accepted"`. The team-execution plan §E rework-lane
///     extension additionally stamps `lane='implement'` + `tier=NULL` on the
///     spawned task, binds it into a sprint (the finding's run target sprint, or
///     a fallback to the host story's existing sprint membership), and bumps the
///     host finding's `rounds` counter — all folded into THIS decision's single
///     event so the rework task re-enters the §C claim queue. `tier=NULL` (NOT a
///     `deep` default) is deliberate: it lets a lite OR deep agent re-claim the
///     rework under the `(:tier IS NULL OR tier=:tier)` filter; a reviewer can
///     force a tier afterward via `set_task_tier`.
///   * `SpawnStory` → create a child `story` under the finding's host work_item;
///     `triage_state = "accepted"`. NOTE (R12): for a queue-RESIDENT finding this
///     verdict is effectively UNREACHABLE — a `story` child needs a `focus`
///     parent (hierarchy trigger), but `get_story_finding_queue` only surfaces
///     findings hosted on a `story` or its `task` children, neither of which can
///     parent a story. SpawnStory is reachable only when a finding is created
///     DIRECTLY on a `focus` work-item.
///   * `Defer` → no spawn; `triage_state = "deferred"`.
///   * `Dismiss` → no spawn; `triage_state = "dismissed"`.
///   * `Resolve` → no spawn; `triage_state = "accepted"`; ALSO resolves the
///     finding terminally (see the delegation note below).
///
/// ## Spawn parent + title
/// A spawn parents the new item under the finding's own host `work_item_id`
/// (a finding with a NULL host cannot parent a child, so a spawn-on-hostless
/// finding is a clean `Validation`). The child's title is the finding's
/// `summary` when present and non-empty, else `"Spawned from finding <id>"`.
/// `create_work_item_full_tx` enforces the hierarchy: a `task` needs a `story`
/// parent, a `story` needs a `focus` parent whose epic carries ≥1
/// close-criterion. An incompatible host kind ⇒ the helper's `Validation`
/// propagates UN-swallowed (the caller must issue a spawn kind that fits the
/// host). The new id is then stamped onto `work_items.spawned_from_finding_id`
/// (mirroring [`create_work_items`]).
///
/// ## Single-mutation-path + the Resolve atomicity (D9, R1)
/// ALL verdicts now run ENTIRELY in ONE tx: the host read (R15 — moved onto the
/// tx so it shares the BEGIN IMMEDIATE writer-lock snapshot with the writes,
/// closing a TOCTOU window), (optional) child create via `create_work_item_full_tx`
/// (the no-event tx helper) + spawn stamp, the `findings.triage_state` UPDATE, the
/// `finding_decisions` INSERT, and EXACTLY ONE export-inert
/// `finding.decision_recorded` event (`aggregate_type="finding"`, keyed by the
/// finding id; R-B4 — never `"work_item"`, even though a spawn created one: the
/// child's create folds into this one event).
///
/// `Resolve` is the documented two-events exception (D9): in addition to the
/// decision event it terminally resolves the finding. R1 INLINES that resolve
/// (the `findings` terminal UPDATE + the `finding.resolved` event) INTO this same
/// decision tx — replicating `resolve_finding`'s body on the tx handle rather
/// than calling the self-committing `resolve_finding(db, …)` before the tx —
/// so the triage UPDATE, the `finding_decisions` INSERT, the terminal resolve,
/// and both events all commit (or roll back) atomically. This preserves the
/// documented TWO-events-for-a-resolve shape (`finding.resolved` +
/// `finding.decision_recorded`) while removing the prior crash window between the
/// two independent commits (which could durably resolve the finding yet lose the
/// audit row).
pub async fn record_finding_decision(
    db: &impl DbClient,
    decision: &NewFindingDecision,
) -> Result<(Uuid, Option<Uuid>), AppError> {
    let finding_id = decision.finding_id.as_str();

    // Map the verdict to (spawn-kind, triage_state). `Resolve` additionally
    // terminally resolves the finding (inlined into the tx below, R1).
    let (spawn_kind, triage_state): (Option<&str>, TriageState) = match decision.decision {
        FindingDecisionKind::SpawnTask => (Some("task"), TriageState::Accepted),
        FindingDecisionKind::SpawnStory => (Some("story"), TriageState::Accepted),
        FindingDecisionKind::Defer => (None, TriageState::Deferred),
        FindingDecisionKind::Dismiss => (None, TriageState::Dismissed),
        FindingDecisionKind::Resolve => (None, TriageState::Accepted),
    };
    let triage_state_str = enum_to_str(triage_state);
    let decision_str = enum_to_str(decision.decision);

    let decision_id = Uuid::now_v7();
    let decision_id_str = decision_id.to_string();

    let mut tx = db.begin().await?;

    // Validate the finding exists and capture its host work_item_id + run_id,
    // ON THE TX (R15) so the read shares the writer-lock snapshot with the writes
    // below (closing a TOCTOU window vs. the prior autocommit read). A missing
    // finding is NotFound (not a dangling-FK 500). Both columns are nullable →
    // read back as Option<String>.
    #[derive(Debug)]
    struct FindingHostRow {
        work_item_id: Option<String>,
        run_id: Option<String>,
    }
    impl<'r, R> sqlx::FromRow<'r, R> for FindingHostRow
    where
        R: sqlx::Row,
        usize: sqlx::ColumnIndex<R>,
        &'r str: sqlx::ColumnIndex<R>,
        Option<String>: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    {
        fn from_row(row: &'r R) -> Result<Self, sqlx::Error> {
            Ok(FindingHostRow {
                work_item_id: row.try_get("work_item_id")?,
                run_id: row.try_get("run_id")?,
            })
        }
    }
    let host_row: FindingHostRow = match crate::db::tx_query_opt::<FindingHostRow>(
        tx.as_mut(),
        "SELECT work_item_id, run_id FROM findings WHERE id = $1",
        args![finding_id.to_owned()],
    )
    .await?
    {
        Some(row) => row,
        None => return Err(AppError::NotFound(format!("finding '{finding_id}' not found"))),
    };
    let host_id = host_row.work_item_id;

    // A spawn needs a host to parent under; a hostless finding cannot spawn.
    if spawn_kind.is_some() && host_id.is_none() {
        return Err(AppError::Validation(format!(
            "cannot spawn from finding '{finding_id}': it has no host work_item to parent under"
        )));
    }

    // 1. (spawn only) create the child under the finding's host, then stamp the
    //    provenance back-link. `create_work_item_full_tx` is the no-event tx
    //    helper, so the child's create folds into THIS decision's single event.
    let spawned_id: Option<Uuid> = if let Some(kind) = spawn_kind {
        let host = host_id
            .as_deref()
            .expect("spawn host presence checked above");
        // Title: the finding's summary when present + non-empty, else a fallback.
        let summary: Option<String> = crate::db::tx_scalar_opt::<String>(
            tx.as_mut(),
            "SELECT summary FROM findings WHERE id = $1",
            args![finding_id.to_owned()],
        )
        .await?;
        let fallback = format!("Spawned from finding {finding_id}");
        let title: &str = match summary.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => &fallback,
        };
        // R5: stamp the child's provenance from the finding's run kind
        // (runs.kind ∈ review|optimise), NOT a hardcoded "review" — a finding
        // raised under an optimise run must not be mislabelled. A finding with no
        // run_id (or whose run row is somehow absent) falls back to "review", the
        // prior default.
        let origin: String = match host_row.run_id.as_deref() {
            Some(rid) => crate::db::tx_scalar_opt::<String>(
                tx.as_mut(),
                "SELECT kind FROM runs WHERE id = $1",
                args![rid.to_owned()],
            )
            .await?
            .unwrap_or_else(|| "review".to_owned()),
            None => "review".to_owned(),
        };
        // An incompatible host kind surfaces the helper's Validation UN-swallowed.
        let new_id = create_work_item_full_tx(
            tx.as_mut(),
            kind,
            Some(host),
            title,
            None,
            CreateOpts {
                origin: Some(origin.as_str()),
                outcome: None,
                shape: None,
            },
        )
        .await?;
        // Stamp the provenance back-link (mirrors `create_work_items`).
        tx.execute(
            "UPDATE work_items SET spawned_from_finding_id = $1 WHERE id = $2",
            args![finding_id.to_owned(), new_id.to_string()],
        )
        .await?;

        // --- Rework-lane extension (team-execution plan §E). -----------------
        // The `spawn_task` verdict on a story-hosted REVIEW finding is the
        // review→rework loop: the spawned task must re-enter the §C claim queue
        // as an `implement`-lane task. (The `spawn_story` verdict is NOT a
        // rework task and gets none of this — it stays lane=NULL.) All three
        // steps below fold into the SAME decision tx and add NO new event: the
        // child's create + every stamp/bind folds into the one
        // `finding.decision_recorded` event recorded below (R-B4), exactly as
        // `complete_task`'s review spawn folds into its one create event.
        if kind == "task" {
            let new_id_str = new_id.to_string();

            // 1. Stamp the rework lane/tier. lane='implement' makes the task
            //    claimable on the Implement lane; tier=NULL (per §E — NOT a
            //    default `deep`) is the explicit "tier unassigned, set later via
            //    set_task_tier" state, so a lite OR deep agent can re-claim it
            //    under the `(:tier IS NULL OR tier=:tier)` claim filter. (A
            //    `deep` default would prejudge the rework and hide it from
            //    lite-tier claims.) "review" is a LANE, never a tier; the rework
            //    task is on the implement lane regardless of the originating
            //    review run. Mirrors `complete_task`'s post-create lane/tier
            //    stamp idiom.
            tx.execute(
                r#"
                UPDATE work_items
                SET lane = 'implement',
                    tier = NULL,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = $1
                "#,
                args![new_id_str.clone()],
            )
            .await?;

            // 2. Bind the rework task into a sprint so the §C claim JOIN (keyed
            //    on `sprint_tasks`) can surface it. Resolution order:
            //      (a) PREFER the finding's run target — when the finding carries
            //          a run_id AND that run targets a sprint (runs.target_kind=
            //          'sprint'), use runs.target_id directly.
            //      (b) FALLBACK to the host story's existing sprint membership —
            //          the DISTINCT sprint_id of the story's sprint-bound tasks.
            //          (This is the path the review→rework loop normally takes:
            //          the review run targets the STORY, not a sprint, so (a)
            //          yields nothing and we inherit the sprint via the story's
            //          already-bound tasks — e.g. the impl task that produced the
            //          finding.)
            //    If NEITHER resolves, the task is left unbound: it is still
            //    lane='implement' but invisible to the claim (harmless — a later
            //    add_tasks_to_sprint can bind it). The bind is idempotent at the
            //    junction (ON CONFLICT DO NOTHING), mirroring `add_tasks_to_sprint`
            //    / `complete_task`.
            let sprint_id: Option<String> = match host_row.run_id.as_deref() {
                Some(rid) => crate::db::tx_scalar_opt::<String>(
                    tx.as_mut(),
                    "SELECT target_id FROM runs WHERE id = $1 AND target_kind = 'sprint'",
                    args![rid.to_owned()],
                )
                .await?,
                None => None,
            };
            let sprint_id: Option<String> = match sprint_id {
                Some(s) => Some(s),
                // Fallback: the host story's existing sprint membership. `host`
                // is the finding's host work_item (the story for a review
                // finding); its sprint-bound task children share the sprint.
                None => crate::db::tx_scalar_opt::<String>(
                    tx.as_mut(),
                    r#"
                    SELECT DISTINCT st.sprint_id
                    FROM sprint_tasks st
                    JOIN work_items t ON t.id = st.task_id
                    WHERE t.parent_id = $1
                    "#,
                    args![host.to_owned()],
                )
                .await?,
            };
            if let Some(sprint) = sprint_id {
                tx.execute(
                    r#"
                    INSERT INTO sprint_tasks (sprint_id, task_id)
                    VALUES ($1, $2)
                    ON CONFLICT(sprint_id, task_id) DO NOTHING
                    "#,
                    args![sprint, new_id_str.clone()],
                )
                .await?;
            }

            // 3. Round-cap counter: increment the host finding's `rounds` (the
            //    review→rework round counter). `rounds` is nullable and written
            //    ONLY at insert today, so COALESCE the NULL to 0 before the bump.
            //    The `rounds >= N` cap that makes the reviewer defer+escalate
            //    instead of spawning another rework is the CONSUMER's logic — we
            //    only MAINTAIN the counter here.
            tx.execute(
                "UPDATE findings SET rounds = COALESCE(rounds, 0) + 1 WHERE id = $1",
                args![finding_id.to_owned()],
            )
            .await?;
        }

        Some(new_id)
    } else {
        None
    };

    // 2. Stamp the mapped triage_state on the finding.
    tx.execute(
        "UPDATE findings SET triage_state = $2 WHERE id = $1",
        args![finding_id.to_owned(), triage_state_str.clone()],
    )
    .await?;

    // 2b. Resolve atomicity (D9 / R1): for the Resolve verdict, inline
    //     `resolve_finding`'s body ON THIS TX — the terminal `status`/`resolved_at`
    //     UPDATE plus the `finding.resolved` event — so the resolve, the triage
    //     UPDATE, and the audit INSERT below all commit together (no crash window
    //     between two independent commits). This is the documented two-events case.
    if matches!(decision.decision, FindingDecisionKind::Resolve) {
        let disposition_str = enum_to_str(Disposition::Fixed);
        tx.execute(
            "UPDATE findings \
             SET status = $2, resolved_at = CURRENT_TIMESTAMP \
             WHERE id = $1",
            args![finding_id.to_owned(), disposition_str.clone()],
        )
        .await?;
        let resolved_payload = serde_json::json!({ "disposition": disposition_str });
        record_event(
            tx.as_mut(),
            "finding",
            finding_id,
            "finding.resolved",
            resolved_payload,
        )
        .await?;
    }

    // 3. Record the append-only decision audit row (decided_at left to DEFAULT).
    tx.execute(
        "INSERT INTO finding_decisions (id, finding_id, decision, spawned_work_item_id, decided_by) \
         VALUES ($1, $2, $3, $4, $5)",
        args![
            decision_id_str.clone(),
            finding_id.to_owned(),
            decision_str.clone(),
            spawned_id.map(|id| id.to_string()),
            decision.decided_by.clone(),
        ],
    )
    .await?;

    // 4. EXACTLY ONE export-inert decision event (R-B4 / R19): aggregate_type=
    //    "finding", keyed by the finding id, NOT "work_item" — even when a spawn
    //    created a work_item, its create folds into this event. The Resolve arm
    //    additionally emitted `finding.resolved` above (the documented D9 two-event
    //    exception).
    let payload = serde_json::json!({
        "decision": decision_str,
        "spawned_work_item_id": spawned_id.map(|id| id.to_string()),
    });
    record_inert_event(
        tx.as_mut(),
        "finding",
        finding_id,
        "finding.decision_recorded",
        payload,
    )
    .await?;

    tx.commit().await?;
    Ok((decision_id, spawned_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connect_in_memory;
    use crate::domain::{FindingDecisionKind, NewFindingDecision, NewRun, NewSprint, RunKind, TargetKind};
    use crate::repo::test_support::*;
    use sqlx::SqlitePool;

    /// Read a `runs` row's `status` (NOT NULL with a column DEFAULT).
    async fn run_status(pool: &SqlitePool, id: &str) -> String {
        sqlx::query_scalar::<_, String>("SELECT status FROM runs WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("select run status")
    }

    /// Count `sprint_tasks` rows for a sprint.
    async fn count_sprint_tasks(pool: &SqlitePool, sprint_id: &str) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sprint_tasks WHERE sprint_id = $1")
            .bind(sprint_id)
            .fetch_one(pool)
            .await
            .expect("count sprint_tasks")
    }

    /// Read a work_item's `spawned_from_finding_id` (nullable column).
    async fn spawned_from(pool: &SqlitePool, id: &str) -> Option<String> {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT spawned_from_finding_id FROM work_items WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("select spawned_from_finding_id")
    }

    /// Count `finding_decisions` rows for a finding.
    async fn count_finding_decisions(pool: &SqlitePool, finding_id: &str) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM finding_decisions WHERE finding_id = $1",
        )
        .bind(finding_id)
        .fetch_one(pool)
        .await
        .expect("count finding_decisions")
    }

    /// Read one finding's `triage_state` (NULL-safe to a sentinel) via the runtime
    /// query API — tests assert DB state with `query_scalar`, never the macros.
    async fn finding_triage_state(pool: &SqlitePool, id: &str) -> Option<String> {
        sqlx::query_scalar::<_, Option<String>>("SELECT triage_state FROM findings WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("select triage_state")
    }

    async fn finding_status(pool: &SqlitePool, id: &str) -> Option<String> {
        sqlx::query_scalar::<_, Option<String>>("SELECT status FROM findings WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("select status")
    }

    /// `create_run` accepts a valid live story target and lands a `runs` row with
    /// the column-default status `'open'`.
    #[tokio::test]
    async fn create_run_accepts_live_story_with_open_status() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        let id = create_run(
            &pool,
            &NewRun {
                kind: RunKind::Review,
                target_id: story.clone(),
                target_kind: TargetKind::Story,
            },
        )
        .await
        .expect("create_run on a live story");

        assert_eq!(run_status(&pool, &id.to_string()).await, "open");
    }

    /// `create_run` rejects a wrong-kind target (a story id passed as a sprint
    /// target), a dangling id, and a tombstoned story — all clean `Validation`.
    #[tokio::test]
    async fn create_run_rejects_invalid_targets() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;

        // wrong kind: a real story id, but declared as a sprint target.
        let wrong_kind = create_run(
            &pool,
            &NewRun {
                kind: RunKind::Review,
                target_id: story.clone(),
                target_kind: TargetKind::Sprint,
            },
        )
        .await;
        assert!(
            matches!(wrong_kind, Err(AppError::Validation(_))),
            "story id under a sprint target is a Validation, got {wrong_kind:?}"
        );

        // dangling id under a story target.
        let dangling = create_run(
            &pool,
            &NewRun {
                kind: RunKind::Optimise,
                target_id: "no-such-id".into(),
                target_kind: TargetKind::Story,
            },
        )
        .await;
        assert!(
            matches!(dangling, Err(AppError::Validation(_))),
            "dangling story target is a Validation, got {dangling:?}"
        );

        // tombstoned story: soft-delete it, then target it.
        delete_work_item(&pool, &story).await.expect("soft-delete story");
        let tombstoned = create_run(
            &pool,
            &NewRun {
                kind: RunKind::Review,
                target_id: story.clone(),
                target_kind: TargetKind::Story,
            },
        )
        .await;
        assert!(
            matches!(tombstoned, Err(AppError::Validation(_))),
            "tombstoned story target is a Validation, got {tombstoned:?}"
        );
    }

    /// `create_sprint` returns an id and the row exists with the default status.
    #[tokio::test]
    async fn create_sprint_inserts_row() {
        let pool = connect_in_memory().await.expect("pool");
        let id = create_sprint(
            &pool,
            &NewSprint {
                title: Some("Sprint 1".into()),
                worktree_id: None,
                predecessor_sprint_id: None,
            },
        )
            .await
            .expect("create_sprint");

        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sprints WHERE id = $1")
            .bind(id.to_string())
            .fetch_one(&pool)
            .await
            .expect("count sprints");
        assert_eq!(count, 1, "the sprint row exists");
    }

    /// `add_tasks_to_sprint`: a second add of the same task counts 0 (junction
    /// dedup via ON CONFLICT DO NOTHING), and the membership is not duplicated.
    #[tokio::test]
    async fn add_tasks_to_sprint_dedups_membership() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T1", None)
            .await
            .expect("legal task")
            .to_string();
        let sprint = seed_sprint(&pool).await;

        let first = add_tasks_to_sprint(&pool, &sprint, &[task.as_str()])
            .await
            .expect("first add");
        assert_eq!(first, 1, "first add inserts one membership");

        let second = add_tasks_to_sprint(&pool, &sprint, &[task.as_str()])
            .await
            .expect("second add");
        assert_eq!(second, 0, "re-adding the same task is a dedup skip, not an error");

        assert_eq!(
            count_sprint_tasks(&pool, &sprint).await,
            1,
            "the membership is not duplicated"
        );
    }

    /// `add_tasks_to_sprint`: a non-task id aborts the whole batch (all-or-nothing)
    /// — no memberships persist, even the valid ones that preceded it.
    #[tokio::test]
    async fn add_tasks_to_sprint_aborts_on_non_task() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let task = create_work_item(&pool, "task", Some(&story), "T1", None)
            .await
            .expect("legal task")
            .to_string();
        let sprint = seed_sprint(&pool).await;

        // The story id is a valid work_item but NOT a task → abort the batch.
        let res = add_tasks_to_sprint(&pool, &sprint, &[task.as_str(), story.as_str()]).await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "a non-task member aborts the batch, got {res:?}"
        );
        assert_eq!(
            count_sprint_tasks(&pool, &sprint).await,
            0,
            "rollback left zero memberships — all-or-nothing"
        );
    }

    /// `record_finding_decision` SpawnTask on a story-hosted finding creates a
    /// child task with `spawned_from_finding_id` set, `triage_state='accepted'`,
    /// and a `finding_decisions` row naming the new id.
    #[tokio::test]
    async fn record_finding_decision_spawn_task() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let finding = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("needs a follow-up task"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        let (decision_id, spawned) = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding.clone(),
                decision: FindingDecisionKind::SpawnTask,
                decided_by: Some("triager".into()),
            },
        )
        .await
        .expect("spawn_task decision");

        let new_id = spawned.expect("spawn_task yields a work_item id").to_string();

        // The spawned item is a task parented under the host story.
        let (kind, parent): (String, Option<String>) = {
            use sqlx::Row as _;
            let r = sqlx::query("SELECT kind, parent_id FROM work_items WHERE id = $1")
                .bind(&new_id)
                .fetch_one(&pool)
                .await
                .unwrap();
            (r.try_get("kind").unwrap(), r.try_get("parent_id").unwrap())
        };
        assert_eq!(kind, "task", "spawned a task");
        assert_eq!(parent.as_deref(), Some(story.as_str()), "parented under the host story");

        assert_eq!(
            spawned_from(&pool, &new_id).await.as_deref(),
            Some(finding.as_str()),
            "spawned_from_finding_id back-link is stamped"
        );
        assert_eq!(
            finding_triage_state(&pool, &finding).await.as_deref(),
            Some("accepted"),
            "spawn_task sets triage_state=accepted"
        );

        // A finding_decisions row exists naming the new id.
        let recorded_spawn: Option<String> = sqlx::query_scalar::<_, Option<String>>(
            "SELECT spawned_work_item_id FROM finding_decisions WHERE id = $1",
        )
        .bind(decision_id.to_string())
        .fetch_one(&pool)
        .await
        .expect("select finding_decisions row");
        assert_eq!(
            recorded_spawn.as_deref(),
            Some(new_id.as_str()),
            "the decision row names the spawned work_item"
        );
    }

    /// `record_finding_decision` Resolve resolves the finding ATOMICALLY in the
    /// SAME tx as the decision audit (R1): the finding ends with a terminal
    /// `status`, `triage_state='accepted'`, a `finding_decisions` row exists, AND
    /// BOTH the `finding.resolved` and `finding.decision_recorded` events are
    /// present (the documented D9 two-event shape) — committed together, so the
    /// audit row can never be lost to a crash between two independent commits as
    /// the prior delegate-to-`resolve_finding` path allowed. No work_item spawned.
    #[tokio::test]
    async fn record_finding_decision_resolve_is_atomic() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let finding = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("already fixed"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        let work_items_before = count_work_items(&pool).await;

        let (_decision_id, spawned) = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding.clone(),
                decision: FindingDecisionKind::Resolve,
                decided_by: None,
            },
        )
        .await
        .expect("resolve decision");

        assert!(spawned.is_none(), "resolve spawns no work_item");
        assert_eq!(
            count_work_items(&pool).await,
            work_items_before,
            "no work_item created by a resolve"
        );
        // Terminal disposition stamped by the inlined resolve.
        assert_eq!(
            finding_status(&pool, &finding).await.as_deref(),
            Some("fixed"),
            "resolve stamps a terminal Fixed disposition"
        );
        assert_eq!(
            finding_triage_state(&pool, &finding).await.as_deref(),
            Some("accepted"),
            "resolve sets triage_state=accepted"
        );
        assert_eq!(
            count_finding_decisions(&pool, &finding).await,
            1,
            "the audit decision row committed atomically with the resolve"
        );
        // BOTH events are present, keyed to the finding id (the D9 two-event shape,
        // now from a SINGLE tx). A crash that lost the decision row would also have
        // rolled back the resolve — they share one commit.
        let resolved_events = count_events_for(&pool, &finding, "finding.resolved").await;
        let decision_events =
            count_events_for(&pool, &finding, "finding.decision_recorded").await;
        assert_eq!(resolved_events, 1, "exactly one finding.resolved event");
        assert_eq!(
            decision_events, 1,
            "exactly one finding.decision_recorded event — same tx as the resolve"
        );
    }

    /// `record_finding_decision` Dismiss sets `triage_state='dismissed'` and
    /// spawns nothing.
    #[tokio::test]
    async fn record_finding_decision_dismiss() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let finding = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("not a real problem"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        let work_items_before = count_work_items(&pool).await;

        let (_decision_id, spawned) = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding.clone(),
                decision: FindingDecisionKind::Dismiss,
                decided_by: None,
            },
        )
        .await
        .expect("dismiss decision");

        assert!(spawned.is_none(), "dismiss spawns no work_item");
        assert_eq!(
            count_work_items(&pool).await,
            work_items_before,
            "no work_item created by a dismiss"
        );
        assert_eq!(
            finding_triage_state(&pool, &finding).await.as_deref(),
            Some("dismissed"),
            "dismiss sets triage_state=dismissed"
        );
    }

    /// R6: `record_finding_decision` SpawnStory on a FOCUS-hosted finding creates
    /// a child `story` under the focus with `spawned_from_finding_id` set and
    /// `triage_state='accepted'`. SpawnStory is unreachable for a queue-resident
    /// (story/task-hosted) finding, so the finding is created directly on a focus.
    #[tokio::test]
    async fn record_finding_decision_spawn_story() {
        let pool = connect_in_memory().await.expect("pool");
        let focus = seed_chain_to_focus(&pool).await;
        let finding = create_finding(
            &pool,
            &focus,
            &NewFinding { summary: Some("needs a follow-up story"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        let (decision_id, spawned) = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding.clone(),
                decision: FindingDecisionKind::SpawnStory,
                decided_by: Some("triager".into()),
            },
        )
        .await
        .expect("spawn_story decision");

        let new_id = spawned.expect("spawn_story yields a work_item id").to_string();

        let (kind, parent): (String, Option<String>) = {
            use sqlx::Row as _;
            let r = sqlx::query("SELECT kind, parent_id FROM work_items WHERE id = $1")
                .bind(&new_id)
                .fetch_one(&pool)
                .await
                .unwrap();
            (r.try_get("kind").unwrap(), r.try_get("parent_id").unwrap())
        };
        assert_eq!(kind, "story", "spawned a story");
        assert_eq!(parent.as_deref(), Some(focus.as_str()), "parented under the host focus");
        assert_eq!(
            spawned_from(&pool, &new_id).await.as_deref(),
            Some(finding.as_str()),
            "spawned_from_finding_id back-link is stamped"
        );
        assert_eq!(
            finding_triage_state(&pool, &finding).await.as_deref(),
            Some("accepted"),
            "spawn_story sets triage_state=accepted"
        );
        let recorded_spawn: Option<String> = sqlx::query_scalar::<_, Option<String>>(
            "SELECT spawned_work_item_id FROM finding_decisions WHERE id = $1",
        )
        .bind(decision_id.to_string())
        .fetch_one(&pool)
        .await
        .expect("select finding_decisions row");
        assert_eq!(
            recorded_spawn.as_deref(),
            Some(new_id.as_str()),
            "the decision row names the spawned story"
        );
    }

    /// R6: `record_finding_decision` Defer sets `triage_state='deferred'`, spawns
    /// nothing, and records a `finding_decisions` audit row.
    #[tokio::test]
    async fn record_finding_decision_defer() {
        let pool = connect_in_memory().await.expect("pool");
        let story = seed_chain_to_story(&pool).await;
        let finding = create_finding(
            &pool,
            &story,
            &NewFinding { summary: Some("later"), ..NewFinding::default() },
        )
        .await
        .expect("finding")
        .to_string();

        let work_items_before = count_work_items(&pool).await;

        let (_decision_id, spawned) = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding.clone(),
                decision: FindingDecisionKind::Defer,
                decided_by: None,
            },
        )
        .await
        .expect("defer decision");

        assert!(spawned.is_none(), "defer spawns no work_item");
        assert_eq!(
            count_work_items(&pool).await,
            work_items_before,
            "no work_item created by a defer"
        );
        assert_eq!(
            finding_triage_state(&pool, &finding).await.as_deref(),
            Some("deferred"),
            "defer sets triage_state=deferred"
        );
        assert_eq!(
            count_finding_decisions(&pool, &finding).await,
            1,
            "a decision audit row is recorded for the defer"
        );
    }

    /// R7(a): `record_finding_decision` against a finding id that names no row is
    /// a clean `NotFound` (not a 500 / dangling-FK).
    #[tokio::test]
    async fn record_finding_decision_missing_finding_is_not_found() {
        let pool = connect_in_memory().await.expect("pool");
        let res = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: "no-such-finding".into(),
                decision: FindingDecisionKind::Dismiss,
                decided_by: None,
            },
        )
        .await;
        assert!(
            matches!(res, Err(AppError::NotFound(_))),
            "a missing finding id is NotFound, got {res:?}"
        );
    }

    /// R7(b): a SPAWN verdict against a hostless finding (NULL `work_item_id`) is a
    /// clean `Validation` — a finding with no host cannot parent a child.
    #[tokio::test]
    async fn record_finding_decision_hostless_spawn_is_validation() {
        let pool = connect_in_memory().await.expect("pool");
        // A hostless finding: insert directly with a NULL work_item_id (the public
        // create paths require a host, so seed the edge case via raw SQL).
        let finding_id = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO findings (id, work_item_id, summary) VALUES ($1, NULL, $2)",
        )
        .bind(&finding_id)
        .bind("hostless")
        .execute(&pool)
        .await
        .expect("insert hostless finding");

        let res = record_finding_decision(
            &pool,
            &NewFindingDecision {
                finding_id: finding_id.clone(),
                decision: FindingDecisionKind::SpawnTask,
                decided_by: None,
            },
        )
        .await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "a spawn on a hostless finding is a Validation, got {res:?}"
        );
    }

    /// `create_sprint` lands a `sprints` row whose `status` is the migration-0016
    /// create-default `'draft'` (written EXPLICITLY — the pre-0016 column DEFAULT
    /// `'open'` is now vestigial). Renamed from the pre-0016
    /// `create_sprint_persists_default_open_status`, which asserted `'open'`.
    #[tokio::test]
    async fn create_sprint_persists_default_draft_status() {
        let pool = connect_in_memory().await.expect("pool");
        let id = create_sprint(
            &pool,
            &NewSprint {
                title: Some("Sprint 1".into()),
                worktree_id: None,
                predecessor_sprint_id: None,
            },
        )
            .await
            .expect("create_sprint")
            .to_string();

        let status = sqlx::query_scalar::<_, String>("SELECT status FROM sprints WHERE id = $1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .expect("select sprint status");
        assert_eq!(status, "draft", "sprint persists the migration-0016 default 'draft' status");
    }

    /// Read a `sprints` row's `status` string via the runtime query API.
    async fn sprint_status(pool: &SqlitePool, id: &str) -> String {
        sqlx::query_scalar::<_, String>("SELECT status FROM sprints WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("select sprint status")
    }

    /// INSERT a `worktrees` row owned by `sprint` directly via the runtime sqlx
    /// API — the guard tests must not depend on task 4's `create_worktree` (lands
    /// in parallel). Returns the new worktree id.
    async fn seed_worktree_owned_by(pool: &SqlitePool, sprint: &str) -> String {
        let wt_id = Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO worktrees (id, owning_sprint_id, path) VALUES ($1, $2, $3)")
            .bind(&wt_id)
            .bind(sprint)
            .bind("/tmp/wt")
            .execute(pool)
            .await
            .expect("insert worktree");
        wt_id
    }

    /// `set_sprint_status` walks the full legal happy path
    /// `draft → ready → active → review → done`, each step succeeding and
    /// persisting the new status.
    #[tokio::test]
    async fn set_sprint_status_legal_path_succeeds() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await; // status starts 'draft'

        for next in [
            SprintStatus::Ready,
            SprintStatus::Active,
            SprintStatus::Review,
            SprintStatus::Done,
        ] {
            set_sprint_status(&pool, &sprint, next)
                .await
                .unwrap_or_else(|e| panic!("legal transition to {next:?} should succeed: {e:?}"));
            assert_eq!(sprint_status(&pool, &sprint).await, enum_to_str(next));
        }
    }

    /// `set_sprint_status` rejects an illegal transition (`draft → done`) with a
    /// clean `Validation`, leaving the status unchanged.
    #[tokio::test]
    async fn set_sprint_status_illegal_transition_is_validation() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await; // 'draft'

        let res = set_sprint_status(&pool, &sprint, SprintStatus::Done).await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "draft → done is illegal, got {res:?}"
        );
        assert_eq!(
            sprint_status(&pool, &sprint).await,
            "draft",
            "an illegal transition leaves the status unchanged"
        );
    }

    /// `set_sprint_status` against a missing sprint id is a clean `NotFound`.
    #[tokio::test]
    async fn set_sprint_status_missing_sprint_is_not_found() {
        let pool = connect_in_memory().await.expect("pool");
        let res = set_sprint_status(&pool, "no-such-sprint", SprintStatus::Ready).await;
        assert!(
            matches!(res, Err(AppError::NotFound(_))),
            "a missing sprint id is NotFound, got {res:?}"
        );
    }

    /// WORKTREE-OWNER GUARD: a worktree-OWNING sprint at `review` is REJECTED from
    /// `review → done` via `set_sprint_status` (the merge audit must go through
    /// record_worktree_merge/rejection). The sprint is walked to `review` along
    /// the legal path, then a `worktrees` row is inserted naming it as owner.
    #[tokio::test]
    async fn set_sprint_status_worktree_owner_terminal_is_rejected() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await;

        // Walk the legal path to 'review' (exercising set_sprint_status).
        for next in [SprintStatus::Ready, SprintStatus::Active, SprintStatus::Review] {
            set_sprint_status(&pool, &sprint, next).await.expect("legal step");
        }
        // Now the sprint OWNS a worktree.
        seed_worktree_owned_by(&pool, &sprint).await;

        let to_done = set_sprint_status(&pool, &sprint, SprintStatus::Done).await;
        assert!(
            matches!(to_done, Err(AppError::Validation(_))),
            "review → done on a worktree-owning sprint must be rejected, got {to_done:?}"
        );
        let to_cancelled = set_sprint_status(&pool, &sprint, SprintStatus::Cancelled).await;
        assert!(
            matches!(to_cancelled, Err(AppError::Validation(_))),
            "review → cancelled on a worktree-owning sprint must be rejected, got {to_cancelled:?}"
        );
        assert_eq!(
            sprint_status(&pool, &sprint).await,
            "review",
            "the rejected terminal transitions leave the sprint at 'review'"
        );
    }

    /// A worktree-LESS sprint at `review` transitions `review → done` normally
    /// (the guard applies only to worktree-OWNING sprints).
    #[tokio::test]
    async fn set_sprint_status_worktreeless_review_to_done_succeeds() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await;
        for next in [SprintStatus::Ready, SprintStatus::Active, SprintStatus::Review] {
            set_sprint_status(&pool, &sprint, next).await.expect("legal step");
        }

        set_sprint_status(&pool, &sprint, SprintStatus::Done)
            .await
            .expect("a worktree-less review → done succeeds");
        assert_eq!(sprint_status(&pool, &sprint).await, "done");
    }

    /// `set_sprint_status` maps an UNRECOGNISED stored status string to a clean
    /// `Validation`, never a parse panic. The bad value is written directly via
    /// the runtime sqlx API (no production path writes it).
    #[tokio::test]
    async fn set_sprint_status_unrecognised_current_is_validation() {
        let pool = connect_in_memory().await.expect("pool");
        let sprint = seed_sprint(&pool).await;
        sqlx::query("UPDATE sprints SET status = 'legacy-open' WHERE id = $1")
            .bind(&sprint)
            .execute(&pool)
            .await
            .expect("force a legacy status");

        let res = set_sprint_status(&pool, &sprint, SprintStatus::Ready).await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "an unrecognised stored status is a Validation, got {res:?}"
        );
    }

    /// `create_sprint` persists `worktree_id` + `predecessor_sprint_id` for a
    /// chained sprint, validating both reference existing rows. The predecessor is
    /// a real sprint; its worktree is inserted directly (no dependency on task 4).
    #[tokio::test]
    async fn create_sprint_persists_chaining_columns() {
        let pool = connect_in_memory().await.expect("pool");
        let predecessor = seed_sprint(&pool).await;
        let worktree = seed_worktree_owned_by(&pool, &predecessor).await;

        let chained = create_sprint(
            &pool,
            &NewSprint {
                title: Some("fix sprint".into()),
                worktree_id: Some(worktree.clone()),
                predecessor_sprint_id: Some(predecessor.clone()),
            },
        )
        .await
        .expect("chained sprint")
        .to_string();

        let (wt, pred): (Option<String>, Option<String>) = {
            use sqlx::Row as _;
            let r = sqlx::query(
                "SELECT worktree_id, predecessor_sprint_id FROM sprints WHERE id = $1",
            )
            .bind(&chained)
            .fetch_one(&pool)
            .await
            .unwrap();
            (r.try_get("worktree_id").unwrap(), r.try_get("predecessor_sprint_id").unwrap())
        };
        assert_eq!(wt.as_deref(), Some(worktree.as_str()), "worktree_id persisted");
        assert_eq!(
            pred.as_deref(),
            Some(predecessor.as_str()),
            "predecessor_sprint_id persisted"
        );
    }

    /// `create_sprint` rejects a dangling `worktree_id` / `predecessor_sprint_id`
    /// with a clean `Validation` (never a dangling-FK 500).
    #[tokio::test]
    async fn create_sprint_rejects_dangling_chaining_refs() {
        let pool = connect_in_memory().await.expect("pool");

        let bad_worktree = create_sprint(
            &pool,
            &NewSprint {
                title: None,
                worktree_id: Some("no-such-worktree".into()),
                predecessor_sprint_id: None,
            },
        )
        .await;
        assert!(
            matches!(bad_worktree, Err(AppError::Validation(_))),
            "a dangling worktree_id is a Validation, got {bad_worktree:?}"
        );

        let bad_pred = create_sprint(
            &pool,
            &NewSprint {
                title: None,
                worktree_id: None,
                predecessor_sprint_id: Some("no-such-sprint".into()),
            },
        )
        .await;
        assert!(
            matches!(bad_pred, Err(AppError::Validation(_))),
            "a dangling predecessor_sprint_id is a Validation, got {bad_pred:?}"
        );
    }
}
