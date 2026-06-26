//! Per-unit DRIVE step (focus 1C.3): the `drive` scheduled-unit kind + the
//! per-STORY `drive_depth` gate (migration 0028).
//!
//! ## What this closes
//! A story carries a nullable `work_items.drive_depth` the grill sets to record
//! how far the scheduler should autonomously drive it:
//!   * `plan-only`      — stop after the planning pass;
//!   * `compose-sprint` — plan, then compose a sprint, then stop;
//!   * `drive-to-merge` — plan, compose, and drive the composed sprint THROUGH to
//!     the OFF-MAIN integration merge, then STOP (it NEVER targets `main`).
//!
//! This module is the engine half that, on each wake, looks at the pending
//! `drive` scheduled units and decides — per unit — whether to drive to the
//! off-main integration merge or to stop short. It is the sibling of `reclaim` /
//! `redispatch`: enable-gated, errors swallowed, sleep-free.
//!
//! ## The SECURITY floor is load-bearing here
//! The drive step targets the OFF-MAIN integration branch (the worktree's
//! `base_ref` of the sprint composed over the story's tasks). It must NEVER
//! target a protected branch — that is the irreversible-merge floor's job, and
//! merging a protected branch autonomously is exactly what the floor forbids. So
//! [`decide_drive`] re-uses the SINGLE-SOURCE [`crate::mcp::is_protected_target`]
//! predicate (the same one `mcp::worktrees`'s `execute_worktree_merge_flow`
//! gates on): a `drive-to-merge` story whose resolved integration target turns
//! out to be protected is REFUSED ([`DriveDecision::StopProtected`]) — the
//! autonomous drive step stops rather than attempt a protected-branch merge. The
//! protected-merge path requires the operator-resolved authorising question and
//! is NOT the drive step's remit.
//!
//! ## What is the load-bearing core, and what is the documented seam
//! LOAD-BEARING (here, tested): the `drive_depth` DEPTH GATE, the OFF-MAIN target
//! selection, and the protected-branch floor compliance — all in the pure
//! [`decide_drive`] + the [`drive_pending_units`] classification pass over real
//! rows. The DOCUMENTED SEAM: actually dispatching the off-main merge through the
//! `lumina-companion` (composing `execute_worktree_merge_flow`) needs the
//! `AppState`/companion handle the scheduler loop does NOT hold (`spawn` takes a
//! pool/notify/enabled only — no `AppState`). So a unit that classifies
//! [`DriveDecision::DriveToMerge`] is counted + logged here; wiring the actual
//! companion dispatch (threading an `AppState`/companion into the loop, or a
//! drive worker analogous to `mcp::scheduler::dispatch_scheduled_unit_flow`) is
//! the orchestration follow-up.
//!
//! Runtime `sqlx::query*` only (no bang macros); CONTROL plane — never shells git.

use lumina_core::args;
use lumina_core::db::{AnyPool, DbClient};

use crate::mcp::is_protected_target;

/// The `work_items.drive_depth` wire literal (migration 0028 CHECK +
/// `domain::DriveDepth::DriveToMerge`) that authorises driving to the merge. Any
/// other value (`plan-only`, `compose-sprint`, NULL, or an unrecognised string)
/// stops short of the merge.
const DRIVE_TO_MERGE: &str = "drive-to-merge";

/// The decision the drive step assigns one pending `drive` scheduled unit. Only
/// [`DriveToMerge`] would proceed to the off-main companion merge (the documented
/// seam); the three `Stop*` variants are no-ops that stop short.
///
/// [`DriveToMerge`]: DriveDecision::DriveToMerge
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DriveDecision {
    /// The DEPTH GATE stops the drive: `drive_depth` is `plan-only`,
    /// `compose-sprint`, unset (NULL), or unrecognised — not `drive-to-merge`.
    StopDepth,
    /// `drive-to-merge`, but NO off-main integration target resolved (no composed
    /// sprint / owned worktree / `base_ref` yet) — nothing to drive.
    StopNoTarget,
    /// `drive-to-merge`, but the resolved target is a PROTECTED branch — the drive
    /// step NEVER targets `main`, so it REFUSES (the operator floor governs a
    /// protected merge; that is not the drive step's job).
    StopProtected {
        /// The protected target the drive refused to merge into.
        target: String,
    },
    /// `drive-to-merge`, an OFF-MAIN target resolved and floor-clean — drive the
    /// composed sprint through to that off-main integration merge (dispatch is the
    /// documented seam).
    DriveToMerge {
        /// The off-main integration branch this unit drives the sprint into.
        target: String,
    },
}

/// The PURE drive decision — the single source of the DEPTH GATE + OFF-MAIN
/// target selection + protected-branch floor compliance, factored out so it is
/// exhaustively unit-testable without a DB.
///
/// Priority: (1) DEPTH GATE — only an explicit `drive-to-merge` proceeds; (2) a
/// resolvable off-main target must exist; (3) FLOOR — that target must NOT be
/// protected (the autonomous drive step never targets `main`).
pub(crate) fn decide_drive(
    drive_depth: Option<&str>,
    integration_target: Option<&str>,
) -> DriveDecision {
    // (1) Depth gate: anything other than 'drive-to-merge' stops short of merge.
    if drive_depth != Some(DRIVE_TO_MERGE) {
        return DriveDecision::StopDepth;
    }
    // (2) Off-main target selection: a non-empty integration target must resolve.
    let Some(target) = integration_target
        .map(str::trim)
        .filter(|t| !t.is_empty())
    else {
        return DriveDecision::StopNoTarget;
    };
    // (3) Floor compliance (single-source predicate): NEVER target a protected
    // branch from the autonomous drive step.
    if is_protected_target(target) {
        return DriveDecision::StopProtected { target: target.to_owned() };
    }
    DriveDecision::DriveToMerge { target: target.to_owned() }
}

/// Per-disposition counts for one drive pass — surfaced for the loop's structured
/// log (and asserted by tests).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DriveOutcome {
    /// Units that would drive to the off-main integration merge (the seam-bound
    /// outcome — dispatch is the documented follow-up).
    pub drive_to_merge: usize,
    /// Units stopped by the DEPTH GATE (not `drive-to-merge`).
    pub stopped_depth: usize,
    /// Units that are `drive-to-merge` but have no off-main target yet.
    pub stopped_no_target: usize,
    /// Units REFUSED because their resolved target is a PROTECTED branch (the
    /// drive step never targets `main`).
    pub stopped_protected: usize,
}

/// One row of the drive SELECT, positionally decoded: `(su.id, w.id,
/// w.drive_depth, integration_target)`.
type DriveTuple = (String, String, Option<String>, Option<String>);

/// Run one DRIVE classification pass over the `kind='drive'`, `status='pending'`
/// scheduled units and return the per-disposition counts.
///
/// For each pending drive unit it reads the driving story's `drive_depth` and the
/// OFF-MAIN integration target — the `base_ref` of the most-recent live worktree
/// owned by a sprint composed over the story's tasks — and applies the pure
/// [`decide_drive`] gate. A [`DriveDecision::DriveToMerge`] is counted + logged;
/// the actual off-main companion merge dispatch is the documented seam (the loop
/// holds no `AppState`/companion). Every error is LOGGED and SWALLOWED — one bad
/// row must not kill the loop, and a query failure returns the zero outcome (fail
/// safe). Read-only: no transaction, no events; sleep-free.
pub async fn drive_pending_units(db: &AnyPool) -> DriveOutcome {
    // ONE classification SELECT: the pending drive unit, its driving story's
    // drive_depth, and a correlated subquery resolving the OFF-MAIN integration
    // target (the base_ref of the most-recent LIVE worktree owned by a sprint
    // composed over the story's tasks). A JOIN (not LEFT) on work_items: the
    // FK guarantees the row exists; a vanished story is the redispatch sibling's
    // STOP concern, not the drive gate's.
    let rows: Vec<DriveTuple> = match db
        .query_all(
            r#"
            SELECT
                su.id,
                w.id,
                w.drive_depth,
                (SELECT wt.base_ref
                   FROM sprint_tasks st
                   JOIN work_items t  ON t.id = st.task_id
                   JOIN sprints     sp ON sp.id = st.sprint_id
                   JOIN worktrees   wt ON wt.id = sp.worktree_id
                  WHERE t.parent_id = w.id
                    AND wt.deleted_at IS NULL
                  ORDER BY wt.created_at DESC, wt.id DESC
                  LIMIT 1) AS integration_target
            FROM scheduled_units su
            JOIN work_items w ON w.id = su.work_item_id
            WHERE su.kind = 'drive' AND su.status = 'pending'
            "#,
            args![],
        )
        .await
    {
        Ok(r) => r,
        Err(err) => {
            tracing::warn!(error = %err, "scheduler drive: classification query failed; skipping pass");
            return DriveOutcome::default();
        }
    };

    let mut outcome = DriveOutcome::default();
    for (unit_id, story_id, drive_depth, integration_target) in rows {
        match decide_drive(drive_depth.as_deref(), integration_target.as_deref()) {
            DriveDecision::StopDepth => {
                outcome.stopped_depth += 1;
                tracing::debug!(
                    unit_id = %unit_id,
                    story_id = %story_id,
                    drive_depth = ?drive_depth,
                    "scheduler drive: depth gate stops short of merge"
                );
            }
            DriveDecision::StopNoTarget => {
                outcome.stopped_no_target += 1;
                tracing::debug!(
                    unit_id = %unit_id,
                    story_id = %story_id,
                    "scheduler drive: drive-to-merge but no off-main integration target resolved yet"
                );
            }
            DriveDecision::StopProtected { target } => {
                outcome.stopped_protected += 1;
                // SECURITY: the autonomous drive step NEVER targets a protected
                // branch — the operator floor governs a protected merge.
                tracing::warn!(
                    unit_id = %unit_id,
                    story_id = %story_id,
                    target = %target,
                    "scheduler drive: REFUSING — resolved integration target is a PROTECTED branch; \
                     the autonomous drive step never targets main (operator floor required)"
                );
            }
            DriveDecision::DriveToMerge { target } => {
                outcome.drive_to_merge += 1;
                // SEAM: the off-main merge dispatch needs the companion/AppState
                // the loop does not hold. Classified + logged here; wiring the
                // companion dispatch is the documented orchestration follow-up.
                tracing::info!(
                    unit_id = %unit_id,
                    story_id = %story_id,
                    target = %target,
                    "scheduler drive: unit ready to drive to OFF-MAIN integration merge \
                     (companion dispatch is the documented seam)"
                );
            }
        }
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::db::{connect_in_memory, AnyPool};
    use lumina_core::domain::{NewSprint, NewWorktree};
    use lumina_core::repo::{
        add_acceptance_criterion, add_tasks_to_sprint, create_sprint, create_work_item,
        create_work_item_full, create_worktree, CreateOpts,
    };
    use uuid::Uuid;

    // ---- Pure decision: the DEPTH GATE + OFF-MAIN selection + floor -----------

    /// The DEPTH GATE: only an explicit `drive-to-merge` proceeds — `plan-only`,
    /// `compose-sprint`, unset (NULL), and an unrecognised value all STOP short of
    /// the merge, even with a perfectly good off-main target available.
    #[test]
    fn decide_drive_depth_gate_stops_short_of_merge() {
        for d in [Some("plan-only"), Some("compose-sprint"), None, Some("bogus")] {
            assert_eq!(
                decide_drive(d, Some("integration")),
                DriveDecision::StopDepth,
                "drive_depth {d:?} must not drive to merge"
            );
        }
    }

    /// `drive-to-merge` with a resolvable OFF-MAIN target selects it for the
    /// drive.
    #[test]
    fn decide_drive_selects_off_main_target() {
        assert_eq!(
            decide_drive(Some("drive-to-merge"), Some("integration")),
            DriveDecision::DriveToMerge { target: "integration".to_owned() }
        );
    }

    /// THE SECURITY PROPERTY: `drive-to-merge` NEVER targets a protected branch —
    /// `main`/`master` (and `refs/heads/` + case variants) are REFUSED as
    /// `StopProtected`, never driven.
    #[test]
    fn decide_drive_never_targets_protected() {
        for t in ["main", "master", "refs/heads/main", "MAIN"] {
            match decide_drive(Some("drive-to-merge"), Some(t)) {
                DriveDecision::StopProtected { target } => assert_eq!(target, t.trim()),
                other => panic!("drive-to-merge into {t} must be refused as protected, got {other:?}"),
            }
        }
    }

    /// `drive-to-merge` with no resolvable target (absent or whitespace-only)
    /// stops as `StopNoTarget`.
    #[test]
    fn decide_drive_stops_without_a_target() {
        assert_eq!(
            decide_drive(Some("drive-to-merge"), None),
            DriveDecision::StopNoTarget
        );
        assert_eq!(
            decide_drive(Some("drive-to-merge"), Some("   ")),
            DriveDecision::StopNoTarget
        );
    }

    // ---- DB classification pass over real rows --------------------------------

    /// Build a real project→epic→focus→story chain (the create-hierarchy gate
    /// requires every level) and return the story id. Mirrors the chain builder in
    /// `scheduler/redispatch.rs`'s tests.
    async fn seed_story(db: &AnyPool) -> String {
        let project = create_work_item(db, "project", None, "P", None)
            .await
            .expect("project")
            .to_string();
        let epic = create_work_item_full(
            db,
            "epic",
            Some(&project),
            "E",
            None,
            CreateOpts { origin: None, outcome: Some("o"), shape: None, lane: None },
        )
        .await
        .expect("epic")
        .to_string();
        add_acceptance_criterion(db, &epic, "epic close criterion")
            .await
            .expect("epic close criterion");
        let focus = create_work_item_full(
            db,
            "focus",
            Some(&epic),
            "FO",
            None,
            CreateOpts { origin: None, outcome: None, shape: Some("vertical-slice"), lane: None },
        )
        .await
        .expect("focus")
        .to_string();
        create_work_item(db, "story", Some(&focus), "S", None)
            .await
            .expect("story")
            .to_string()
    }

    /// Seed ONE pending `drive` scheduled unit over `work_item_id` (raw runtime
    /// sqlx — seeding is allowed, NOT a bang macro). Returns the unit id.
    async fn seed_drive_unit(db: &AnyPool, work_item_id: &str) -> String {
        let unit_id = Uuid::now_v7().to_string();
        db.execute(
            "INSERT INTO scheduled_units (id, kind, work_item_id, status) \
             VALUES ($1, 'drive', $2, 'pending')",
            args![unit_id.clone(), work_item_id.to_owned()],
        )
        .await
        .expect("seed drive scheduled_unit");
        unit_id
    }

    /// Set a story's `drive_depth` (raw UPDATE — there is no setter mutator yet;
    /// migration 0028 only added the column).
    async fn set_drive_depth(db: &AnyPool, story_id: &str, depth: &str) {
        db.execute(
            "UPDATE work_items SET drive_depth = $2 WHERE id = $1",
            args![story_id.to_owned(), depth.to_owned()],
        )
        .await
        .expect("set drive_depth");
    }

    /// Set the worktree's `base_ref` (the integration target the drive resolves).
    async fn set_worktree_base_ref(db: &AnyPool, worktree_id: &str, base_ref: &str) {
        db.execute(
            "UPDATE worktrees SET base_ref = $2 WHERE id = $1",
            args![worktree_id.to_owned(), base_ref.to_owned()],
        )
        .await
        .expect("set worktree base_ref");
    }

    /// **The drive gate over real rows.** A `drive-to-merge` story whose composed
    /// sprint owns an OFF-MAIN worktree drives to that off-main target; a
    /// `plan-only` story does NOT; and a `drive-to-merge` story whose target is
    /// `main` is REFUSED (never driven).
    #[tokio::test]
    async fn drive_pending_units_gates_on_depth_and_off_main_target() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let story = seed_story(&db).await;
        let task = create_work_item(&db, "task", Some(&story), "T", None)
            .await
            .expect("task")
            .to_string();

        // A sprint composed over the story's task, owning an OFF-MAIN worktree.
        let sprint = create_sprint(
            &db,
            &NewSprint { title: None, worktree_id: None, predecessor_sprint_id: None },
        )
        .await
        .expect("sprint")
        .to_string();
        add_tasks_to_sprint(&db, &sprint, &[task.as_str()]).await.expect("attach task");
        let worktree = create_worktree(
            &db,
            &NewWorktree {
                owning_sprint_id: sprint.clone(),
                path: "/tmp/wt".to_owned(),
                base_ref: Some("integration".to_owned()),
                branch: Some("sprint/1".to_owned()),
            },
        )
        .await
        .expect("worktree")
        .to_string();

        let _unit = seed_drive_unit(&db, &story).await;

        // (a) drive-to-merge + off-main target → drives to off-main.
        set_drive_depth(&db, &story, "drive-to-merge").await;
        let outcome = drive_pending_units(&db).await;
        assert_eq!(outcome.drive_to_merge, 1, "off-main drive-to-merge drives");
        assert_eq!(
            outcome.stopped_depth + outcome.stopped_no_target + outcome.stopped_protected,
            0,
            "no stop disposition for the clean off-main drive"
        );

        // (b) plan-only → DEPTH GATE stops short of merge.
        set_drive_depth(&db, &story, "plan-only").await;
        let outcome = drive_pending_units(&db).await;
        assert_eq!(outcome.stopped_depth, 1, "plan-only does not drive to merge");
        assert_eq!(outcome.drive_to_merge, 0, "plan-only never reaches the merge");

        // (c) drive-to-merge but the target is `main` → REFUSED (never driven).
        set_drive_depth(&db, &story, "drive-to-merge").await;
        set_worktree_base_ref(&db, &worktree, "main").await;
        let outcome = drive_pending_units(&db).await;
        assert_eq!(outcome.stopped_protected, 1, "a main target is refused, not driven");
        assert_eq!(outcome.drive_to_merge, 0, "the drive step never targets main");
    }
}
