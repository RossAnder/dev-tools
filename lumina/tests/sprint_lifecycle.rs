//! Sprint-lifecycle & worktree guard suite (migration 0016, ADR-0002 layer 2,
//! plan T13 of `docs/plans/sprint-lifecycle-worktree-substrate.md`).
//!
//! This is the dedicated integration-test home for the layer-2 lifecycle GUARDS
//! that span `repo::set_sprint_status`, `repo::claim_next_task`, and the
//! `repo::record_worktree_*` audit path. The repo modules carry their own
//! `#[cfg(test)] mod tests` for the unit-level shapes; this file asserts the
//! cross-cutting INVARIANTS an integration consumer relies on:
//!
//!   * the `SprintStatus` legal-transition table is enforced
//!     (`draft→done` / `active→draft` are `Validation`);
//!   * `claim_next_task` is runnable ⟺ the sprint is `'active'` (every other
//!     status — `draft`/`ready`/`review`/`cancelled` — yields `Ok(None)`);
//!   * a checkpoint task in `in_progress` FREEZES its whole sprint (the claim
//!     yields `Ok(None)` until the checkpoint leaves `in_progress`);
//!   * the worktree merge/rejection audit flips the owner `review→done` /
//!     `review→cancelled`, is consistent on a repeated call, and `effective_status`
//!     is JOIN-derived from the owner;
//!   * a worktree-OWNING sprint cannot terminal-transition via `set_sprint_status`
//!     (merge/rejection is the only path);
//!   * lumina is RECORD-ONLY — the whole worktree create→merge lifecycle runs
//!     purely on DB state with no git repository / working tree present.
//!
//! ## Determinism — no sleeps
//!
//! Matching the crate-wide no-flaky-time rule (and `tests/claim_concurrency.rs`):
//! nothing here waits on a TTL. The lease-bearing helpers stamp a far-future
//! deadline so a claimed/in-progress row stays leased for the test's duration;
//! no `tokio::time::sleep`, no countdown.
//!
//! All assertions use the RUNTIME `sqlx::query`/`query_scalar` string API (NOT
//! the compile-time macros), so this test adds no `.sqlx/` cache entry — matching
//! `tests/claim_concurrency.rs` / `tests/migration_0013.rs`. The lib + the
//! migration-0016 repo code have landed; this file only drives them.

use lumina::db::connect_in_memory;
use lumina::domain::{Lane, NewWorktree, SprintStatus, WorktreeOutcome};
use lumina::error::AppError;
use lumina::repo::{self, CreateOpts};
use sqlx::SqlitePool;

/// Read a worktree's audit `outcome` string (NULL when no verdict recorded), via
/// runtime sqlx — used to confirm a rejected NotFound call stamped NO audit.
async fn worktree_outcome(pool: &SqlitePool, worktree_id: &str) -> Option<String> {
    sqlx::query_scalar::<_, Option<String>>("SELECT outcome FROM worktrees WHERE id = $1")
        .bind(worktree_id)
        .fetch_optional(pool)
        .await
        .expect("select worktree outcome")
        .flatten()
}

/// Generous lease TTL (seconds): a claimed/seeded-in-progress row stays leased
/// for the whole test. No test here relies on a lease expiring (no-sleep rule).
const LEASE_TTL_SECS: i64 = 1800;

// ===========================================================================
// Seed helpers — the legal hierarchy chain + a sprint + a queue-ready task,
// reconstructed through the PUBLIC repo API (the repo's `#[cfg(test)]`-private
// `seed_*` helpers are invisible to an integration test). Mirrors the helpers
// in `tests/claim_concurrency.rs`.
// ===========================================================================

/// Build the legal `project → epic → focus → story` chain via the public repo
/// API and return the story id (same shape as
/// `claim_concurrency.rs::seed_chain_to_story`).
async fn seed_chain_to_story(pool: &SqlitePool) -> String {
    let project = repo::create_work_item(pool, "project", None, "P", None)
        .await
        .expect("legal project");
    let epic = repo::create_work_item_full(
        pool,
        "epic",
        Some(&project.to_string()),
        "E",
        None,
        CreateOpts {
            origin: None,
            outcome: Some("the epic outcome"),
            shape: None,
        },
    )
    .await
    .expect("legal epic");
    repo::add_acceptance_criterion(pool, &epic.to_string(), "epic close criterion")
        .await
        .expect("epic close criterion");
    let focus = repo::create_work_item_full(
        pool,
        "focus",
        Some(&epic.to_string()),
        "FO",
        None,
        CreateOpts {
            origin: None,
            outcome: None,
            shape: Some("vertical-slice"),
        },
    )
    .await
    .expect("legal focus");
    let story = repo::create_work_item(pool, "story", Some(&focus.to_string()), "S", None)
        .await
        .expect("legal story");
    story.to_string()
}

/// Open a sprint via the public repo API; returns its id. `create_sprint`
/// stamps the migration-0016 create-default `status='draft'`.
async fn seed_sprint(pool: &SqlitePool) -> String {
    repo::create_sprint(
        pool,
        &lumina::domain::NewSprint {
            title: Some("S1".into()),
            worktree_id: None,
            predecessor_sprint_id: None,
        },
    )
    .await
    .expect("legal sprint")
    .to_string()
}

/// Create an `implement`-lane `task` under `story`, move it to the queue-ready
/// `status='todo'`, and bind it to `sprint`. Returns the task id. `lane`/`status`
/// have no public repo mutator, so they are stamped with a raw runtime
/// `sqlx::query` UPDATE — the same seeding idiom `tests/claim_concurrency.rs`
/// uses (raw runtime sqlx is permitted for seeding; it is NOT a compile-time
/// macro, so the macro-eradication gate stays at 0).
async fn seed_queue_task(pool: &SqlitePool, story: &str, sprint: &str, title: &str) -> String {
    let task = repo::create_work_item(pool, "task", Some(story), title, None)
        .await
        .expect("legal task")
        .to_string();
    sqlx::query("UPDATE work_items SET lane = 'implement', status = 'todo' WHERE id = $1")
        .bind(&task)
        .execute(pool)
        .await
        .expect("stamp lane/status");
    repo::add_tasks_to_sprint(pool, sprint, &[task.as_str()])
        .await
        .expect("bind task to sprint");
    task
}

/// Build a `NewWorktree` owned by `sprint`, with a representative path. NB the
/// path is a bare string — no directory is ever created on disk; lumina records
/// it verbatim (record-only), so a non-existent path is fine.
fn new_worktree(sprint: &str) -> NewWorktree {
    NewWorktree {
        owning_sprint_id: sprint.to_owned(),
        path: "/nonexistent/wt".to_owned(),
        base_ref: Some("main".to_owned()),
        branch: Some("sprint/1".to_owned()),
    }
}

/// Read a sprint's `status` string (runtime sqlx).
async fn sprint_status(pool: &SqlitePool, sprint_id: &str) -> String {
    sqlx::query_scalar::<_, String>("SELECT status FROM sprints WHERE id = $1")
        .bind(sprint_id)
        .fetch_one(pool)
        .await
        .expect("select sprint status")
}

/// Walk a freshly-seeded (`draft`) sprint to `active` along the legal path
/// `draft → ready → active`, exercising `set_sprint_status` at each step.
async fn activate_sprint(pool: &SqlitePool, sprint_id: &str) {
    for next in [SprintStatus::Ready, SprintStatus::Active] {
        repo::set_sprint_status(pool, sprint_id, next)
            .await
            .unwrap_or_else(|e| panic!("legal step to {next:?}: {e:?}"));
    }
}

// ===========================================================================
// 1. Illegal sprint transitions are rejected (Validation), legal ones pass.
// ===========================================================================

/// `set_sprint_status` rejects illegal transitions (`draft→done`, `active→draft`)
/// with a clean `Validation`, leaving the status unchanged; the legal happy path
/// `draft→ready→active→review→done` succeeds end-to-end.
#[tokio::test]
async fn illegal_sprint_transitions_are_validation() {
    let pool = connect_in_memory().await.expect("pool");
    let sprint = seed_sprint(&pool).await; // 'draft'

    // draft → done is illegal (skips the whole lifecycle).
    let draft_to_done = repo::set_sprint_status(&pool, &sprint, SprintStatus::Done).await;
    assert!(
        matches!(draft_to_done, Err(AppError::Validation(_))),
        "draft → done is illegal, got {draft_to_done:?}"
    );
    assert_eq!(
        sprint_status(&pool, &sprint).await,
        "draft",
        "an illegal transition leaves the status unchanged"
    );

    // Advance to 'active' (legal), then active → draft is illegal (no going back).
    activate_sprint(&pool, &sprint).await;
    let active_to_draft = repo::set_sprint_status(&pool, &sprint, SprintStatus::Draft).await;
    assert!(
        matches!(active_to_draft, Err(AppError::Validation(_))),
        "active → draft is illegal, got {active_to_draft:?}"
    );
    assert_eq!(
        sprint_status(&pool, &sprint).await,
        "active",
        "the rejected active → draft leaves the sprint at 'active'"
    );

    // The full legal happy path runs to a terminal `done`.
    for next in [SprintStatus::Review, SprintStatus::Done] {
        repo::set_sprint_status(&pool, &sprint, next)
            .await
            .unwrap_or_else(|e| panic!("legal transition to {next:?}: {e:?}"));
    }
    assert_eq!(sprint_status(&pool, &sprint).await, "done");
}

// ===========================================================================
// 2. claim_next_task is runnable ⟺ the sprint is 'active'.
// ===========================================================================

/// `claim_next_task` refuses every NON-`active` sprint status (`draft`/`ready`/
/// `review`/`cancelled` all yield `Ok(None)`), and claims under `active`. A
/// queue-ready implement task is present throughout, so the `None` is the
/// SPRINT-status guard, not an empty queue.
#[tokio::test]
async fn claim_runs_only_under_active_sprint() {
    let pool = connect_in_memory().await.expect("pool");
    let story = seed_chain_to_story(&pool).await;
    let sprint = seed_sprint(&pool).await; // 'draft'
    let task = seed_queue_task(&pool, &story, &sprint, "T").await;

    // draft → no claim.
    let drafted = repo::claim_next_task(&pool, &sprint, Lane::Implement, None, "agent-a", LEASE_TTL_SECS)
        .await
        .expect("claim runs");
    assert!(drafted.is_none(), "a draft sprint is non-runnable ⇒ Ok(None)");

    // ready → no claim.
    repo::set_sprint_status(&pool, &sprint, SprintStatus::Ready)
        .await
        .expect("draft → ready");
    let readied = repo::claim_next_task(&pool, &sprint, Lane::Implement, None, "agent-a", LEASE_TTL_SECS)
        .await
        .expect("claim runs");
    assert!(readied.is_none(), "a ready sprint is non-runnable ⇒ Ok(None)");

    // active → the task IS claimed.
    repo::set_sprint_status(&pool, &sprint, SprintStatus::Active)
        .await
        .expect("ready → active");
    let claimed = repo::claim_next_task(&pool, &sprint, Lane::Implement, None, "agent-a", LEASE_TTL_SECS)
        .await
        .expect("claim runs")
        .expect("an active sprint claims the ready task");
    assert_eq!(claimed.task_id, task, "the ready task is claimed under 'active'");
    assert_eq!(claimed.assignee, "agent-a");

    // The claim leased the row in_progress (release it back so the later
    // review-status check exercises the SPRINT guard, not an empty queue).
    let released = repo::release_task(&pool, &task, "agent-a")
        .await
        .expect("release the claimed task");
    assert!(released, "the owner releases its claimed task back to the queue");

    // review → no claim (work is done; awaiting a merge decision, not dispatch).
    repo::set_sprint_status(&pool, &sprint, SprintStatus::Review)
        .await
        .expect("active → review");
    let in_review = repo::claim_next_task(&pool, &sprint, Lane::Implement, None, "agent-a", LEASE_TTL_SECS)
        .await
        .expect("claim runs");
    assert!(in_review.is_none(), "a review sprint is non-runnable ⇒ Ok(None)");

    // cancelled (a terminal status) → no claim. review → cancelled is legal for
    // a worktree-LESS sprint (no worktree owned here).
    repo::set_sprint_status(&pool, &sprint, SprintStatus::Cancelled)
        .await
        .expect("review → cancelled (worktree-less)");
    let cancelled = repo::claim_next_task(&pool, &sprint, Lane::Implement, None, "agent-a", LEASE_TTL_SECS)
        .await
        .expect("claim runs");
    assert!(cancelled.is_none(), "a cancelled sprint is non-runnable ⇒ Ok(None)");
}

// ===========================================================================
// 3. Checkpoint-freeze: an in_progress checkpoint task freezes its sprint.
// ===========================================================================

/// A checkpoint task (`work_items.checkpoint=1`) that is `in_progress` FREEZES
/// its whole sprint — the claim yields `Ok(None)` even though another ready
/// implement task exists. Once the checkpoint leaves `in_progress` (→ `done`),
/// the claim resumes and the other task becomes claimable.
#[tokio::test]
async fn in_progress_checkpoint_freezes_then_resumes() {
    let pool = connect_in_memory().await.expect("pool");
    let story = seed_chain_to_story(&pool).await;
    let sprint = seed_sprint(&pool).await;
    activate_sprint(&pool, &sprint).await; // draft → ready → active

    // A checkpoint task and an ordinary ready implement task, both in the sprint.
    let checkpoint = seed_queue_task(&pool, &story, &sprint, "CHECKPOINT").await;
    repo::set_task_checkpoint(&pool, &checkpoint, true)
        .await
        .expect("mark the task a checkpoint");
    let other = seed_queue_task(&pool, &story, &sprint, "OTHER").await;

    // Drive the checkpoint task in_progress directly (a far-future lease so it
    // stays in_progress for the test — no sleep). This is the sprint-wide barrier.
    sqlx::query(
        "UPDATE work_items SET status = 'in_progress', assignee = 'agent-cp', \
         lease_expires_at = datetime('now', '+1800 seconds') WHERE id = $1",
    )
    .bind(&checkpoint)
    .execute(&pool)
    .await
    .expect("checkpoint in_progress");

    // FROZEN: with the checkpoint in_progress, NOTHING is claimable — not even
    // the ordinary `other` task.
    let frozen = repo::claim_next_task(&pool, &sprint, Lane::Implement, None, "agent-a", LEASE_TTL_SECS)
        .await
        .expect("claim runs");
    assert!(
        frozen.is_none(),
        "an in_progress checkpoint freezes the whole sprint ⇒ Ok(None)"
    );

    // The checkpoint leaves in_progress (→ done): the freeze lifts.
    sqlx::query("UPDATE work_items SET status = 'done', assignee = NULL, lease_expires_at = NULL WHERE id = $1")
        .bind(&checkpoint)
        .execute(&pool)
        .await
        .expect("checkpoint done");

    let resumed = repo::claim_next_task(&pool, &sprint, Lane::Implement, None, "agent-a", LEASE_TTL_SECS)
        .await
        .expect("claim runs")
        .expect("the freeze lifted ⇒ the other task is claimable");
    assert_eq!(
        resumed.task_id, other,
        "with the checkpoint done, the ordinary task is claimed"
    );
}

// ===========================================================================
// 4. Worktree merge audit: flips owner review→done; a repeated call is
//    consistent (the second is a no-op-or-rejection that never mutates the
//    already-stamped audit).
// ===========================================================================

/// `record_worktree_merge` on a `review` owner stamps `merged_at`/`merge_ref`/
/// `outcome='merged'` and flips the owner `review → done`. A SECOND merge call is
/// consistent: because the owner is now `done` (no longer `review`), the audit
/// guard rejects it as `Validation` and the already-stamped audit + owner status
/// are unchanged — the merge audit can never be double-applied or corrupted.
#[tokio::test]
async fn merge_flips_owner_to_done_and_is_consistent_on_repeat() {
    let pool = connect_in_memory().await.expect("pool");
    let sprint = seed_sprint(&pool).await;
    let wt = repo::create_worktree(&pool, &new_worktree(&sprint))
        .await
        .expect("create_worktree")
        .to_string();

    // The merge audit is only meaningful once the owner is in 'review'.
    activate_sprint(&pool, &sprint).await;
    repo::set_sprint_status(&pool, &sprint, SprintStatus::Review)
        .await
        .expect("active → review");

    repo::record_worktree_merge(&pool, &wt, Some("merge-ref-xyz"))
        .await
        .expect("merge on a 'review' owner");

    let got = repo::get_worktree(&pool, &wt).await.expect("get_worktree");
    assert_eq!(got.outcome, Some(WorktreeOutcome::Merged), "outcome stamped merged");
    assert_eq!(got.merge_ref.as_deref(), Some("merge-ref-xyz"), "merge_ref stamped");
    assert!(got.merged_at.is_some(), "merged_at stamped");
    assert_eq!(
        sprint_status(&pool, &sprint).await,
        "done",
        "owner flipped 'review' → 'done'"
    );
    // effective_status is JOIN-derived from the now-done owner.
    assert_eq!(got.effective_status, SprintStatus::Done);

    // A SECOND merge is rejected (owner is no longer 'review') and changes
    // nothing — the audit is idempotent/consistent, never double-applied.
    let merged_at_before = got.merged_at.clone();
    let repeat = repo::record_worktree_merge(&pool, &wt, Some("a-different-ref")).await;
    assert!(
        matches!(repeat, Err(AppError::Validation(_))),
        "a repeated merge on a now-'done' owner is rejected, got {repeat:?}"
    );
    let after = repo::get_worktree(&pool, &wt).await.expect("get_worktree");
    assert_eq!(after.outcome, Some(WorktreeOutcome::Merged), "outcome unchanged");
    assert_eq!(
        after.merge_ref.as_deref(),
        Some("merge-ref-xyz"),
        "the original merge_ref is NOT overwritten by the rejected repeat"
    );
    assert_eq!(after.merged_at, merged_at_before, "merged_at unchanged");
    assert_eq!(
        sprint_status(&pool, &sprint).await,
        "done",
        "owner stays 'done' — the rejected repeat did not re-transition it"
    );
}

// ===========================================================================
// 5. Worktree rejection audit: flips owner review→cancelled, outcome=rejected.
// ===========================================================================

/// `record_worktree_rejection` on a `review` owner stamps `outcome='rejected'`
/// (leaving `merged_at` NULL — the decision instant is `updated_at` + the
/// `worktree.rejected` event, not the merge-only `merged_at`) and flips the
/// owner `review → cancelled`; `effective_status` follows the owner.
#[tokio::test]
async fn rejection_flips_owner_to_cancelled() {
    let pool = connect_in_memory().await.expect("pool");
    let sprint = seed_sprint(&pool).await;
    let wt = repo::create_worktree(&pool, &new_worktree(&sprint))
        .await
        .expect("create_worktree")
        .to_string();
    activate_sprint(&pool, &sprint).await;
    repo::set_sprint_status(&pool, &sprint, SprintStatus::Review)
        .await
        .expect("active → review");

    repo::record_worktree_rejection(&pool, &wt, Some("conflicts unresolved"))
        .await
        .expect("rejection on a 'review' owner");

    let got = repo::get_worktree(&pool, &wt).await.expect("get_worktree");
    assert_eq!(got.outcome, Some(WorktreeOutcome::Rejected), "outcome stamped rejected");
    assert!(
        got.merged_at.is_none(),
        "rejection leaves merged_at NULL — it is not a merge (R11)"
    );
    assert_eq!(
        sprint_status(&pool, &sprint).await,
        "cancelled",
        "owner flipped 'review' → 'cancelled'"
    );
    assert_eq!(got.effective_status, SprintStatus::Cancelled);
}

// ===========================================================================
// 6. effective_status is wholly DERIVED from the owner.
// ===========================================================================

/// A worktree has NO `status` column — `effective_status` is JOIN-derived from
/// the owning sprint. Changing the owner's status (via the legal lifecycle) is
/// reflected by `get_worktree` with NO worktree write.
#[tokio::test]
async fn effective_status_derives_from_owner() {
    let pool = connect_in_memory().await.expect("pool");
    let sprint = seed_sprint(&pool).await; // 'draft'
    let wt = repo::create_worktree(&pool, &new_worktree(&sprint))
        .await
        .expect("create_worktree")
        .to_string();

    // At create, the owner is 'draft' → effective_status draft.
    let at_draft = repo::get_worktree(&pool, &wt).await.expect("get_worktree");
    assert_eq!(at_draft.effective_status, SprintStatus::Draft);
    assert_eq!(at_draft.owning_sprint_id, sprint);

    // Walk the owner draft → ready → active; the worktree follows each step with
    // no worktree write of its own.
    repo::set_sprint_status(&pool, &sprint, SprintStatus::Ready)
        .await
        .expect("draft → ready");
    assert_eq!(
        repo::get_worktree(&pool, &wt).await.expect("get").effective_status,
        SprintStatus::Ready,
        "effective_status tracks the owner to 'ready'"
    );

    repo::set_sprint_status(&pool, &sprint, SprintStatus::Active)
        .await
        .expect("ready → active");
    assert_eq!(
        repo::get_worktree(&pool, &wt).await.expect("get").effective_status,
        SprintStatus::Active,
        "effective_status tracks the owner to 'active' — JOIN-derived, no worktree write"
    );
}

// ===========================================================================
// 7. A worktree-OWNING sprint cannot terminal-transition via set_sprint_status.
// ===========================================================================

/// A worktree-OWNING sprint at `review` is REJECTED from `review → done` (and
/// `review → cancelled`) via `set_sprint_status`: the only path to a terminal
/// status for such a sprint is the merge/rejection AUDIT
/// (`record_worktree_merge` / `record_worktree_rejection`). The bare status flip
/// is a clean `Validation` and leaves the sprint at `review`; the audit path then
/// succeeds.
#[tokio::test]
async fn worktree_owner_cannot_terminal_transition_via_set_status() {
    let pool = connect_in_memory().await.expect("pool");
    let sprint = seed_sprint(&pool).await;
    // The sprint OWNS a worktree.
    let wt = repo::create_worktree(&pool, &new_worktree(&sprint))
        .await
        .expect("create_worktree")
        .to_string();

    activate_sprint(&pool, &sprint).await;
    repo::set_sprint_status(&pool, &sprint, SprintStatus::Review)
        .await
        .expect("active → review (non-terminal, allowed)");

    // review → done via set_sprint_status is REJECTED (must go through the merge
    // audit because the sprint owns a worktree).
    let to_done = repo::set_sprint_status(&pool, &sprint, SprintStatus::Done).await;
    assert!(
        matches!(to_done, Err(AppError::Validation(_))),
        "review → done on a worktree-owning sprint must be rejected, got {to_done:?}"
    );
    // review → cancelled via set_sprint_status is likewise REJECTED.
    let to_cancelled = repo::set_sprint_status(&pool, &sprint, SprintStatus::Cancelled).await;
    assert!(
        matches!(to_cancelled, Err(AppError::Validation(_))),
        "review → cancelled on a worktree-owning sprint must be rejected, got {to_cancelled:?}"
    );
    assert_eq!(
        sprint_status(&pool, &sprint).await,
        "review",
        "the rejected terminal transitions leave the sprint at 'review'"
    );

    // The merge AUDIT is the sanctioned path to terminal — and it succeeds.
    repo::record_worktree_merge(&pool, &wt, Some("only-via-merge"))
        .await
        .expect("the merge audit is the only path to terminal for a worktree owner");
    assert_eq!(
        sprint_status(&pool, &sprint).await,
        "done",
        "the merge audit took the worktree-owning sprint terminal"
    );
}

// ===========================================================================
// 8. "lumina never touches git" — the whole worktree lifecycle is record-only.
// ===========================================================================

/// The full worktree create → merge lifecycle runs PURELY on DB state against an
/// in-memory database, with NO git repository, NO working tree, and a path that
/// does not exist on disk. lumina is RECORD-ONLY — it never shells out to git or
/// touches the filesystem — so the merge/rejection AUDIT succeeds entirely on
/// the strength of the recorded rows.
///
/// We cannot directly assert "no shell-out"; the meaningful, observable assertion
/// is that the whole lifecycle works with no `.git` present and a non-existent
/// `path`, stamping only DB rows (the audit `outcome`/`merged_at`, the owner's
/// terminal status). If any step shelled out to git against `path`, it would fail
/// here — there is no repository at `/nonexistent/...`.
#[tokio::test]
async fn worktree_lifecycle_is_record_only_no_git_present() {
    // In-memory DB: no filesystem-backed DB file, and certainly no git repo.
    let pool = connect_in_memory().await.expect("pool");
    let sprint = seed_sprint(&pool).await;

    // A worktree whose `path` points at a directory that does NOT exist on disk.
    let wt_spec = NewWorktree {
        owning_sprint_id: sprint.clone(),
        // A path with no repository / working tree behind it. A record-only
        // implementation accepts it verbatim; a git-shelling one would choke.
        path: "/this/path/does/not/exist/and/has/no/dot-git".to_owned(),
        base_ref: Some("main".to_owned()),
        branch: Some("sprint/record-only".to_owned()),
    };
    let wt = repo::create_worktree(&pool, &wt_spec)
        .await
        .expect("create_worktree records the row regardless of any on-disk git state")
        .to_string();

    // The created worktree reflects exactly what we recorded — path verbatim, no
    // canonicalisation, no on-disk probe.
    let created = repo::get_worktree(&pool, &wt).await.expect("get_worktree");
    assert_eq!(
        created.path, "/this/path/does/not/exist/and/has/no/dot-git",
        "path is recorded verbatim — never canonicalised or probed on disk"
    );
    assert!(created.outcome.is_none(), "no merge audit yet");

    // Drive the owner to 'review' and record the merge AUDIT — purely DB-state.
    activate_sprint(&pool, &sprint).await;
    repo::set_sprint_status(&pool, &sprint, SprintStatus::Review)
        .await
        .expect("active → review");
    repo::record_worktree_merge(&pool, &wt, Some("recorded-merge-sha"))
        .await
        .expect("the merge audit is record-only — it makes ZERO git/filesystem calls");

    // The lifecycle completed entirely on DB state: the audit is stamped and the
    // owner is terminal, with no git repository ever present.
    let merged = repo::get_worktree(&pool, &wt).await.expect("get_worktree");
    assert_eq!(merged.outcome, Some(WorktreeOutcome::Merged), "merge audit stamped");
    assert_eq!(merged.merge_ref.as_deref(), Some("recorded-merge-sha"));
    assert!(merged.merged_at.is_some(), "merged_at stamped");
    assert_eq!(merged.effective_status, SprintStatus::Done);
    assert_eq!(
        sprint_status(&pool, &sprint).await,
        "done",
        "the owner reached terminal 'done' through the record-only audit path alone"
    );
}

// ===========================================================================
// 9. require_review_owner NotFound arm (R21): an ABSENT worktree id is a clean
//    NotFound on BOTH the merge and rejection audit paths, stamping no audit.
// ===========================================================================

/// `record_worktree_merge` and `record_worktree_rejection` on a worktree id that
/// was NEVER created (or was soft-deleted) take the `require_review_owner`
/// missing-owner branch and return `AppError::NotFound` — distinct from the
/// `Validation` arm (a live worktree whose owner is not in `'review'`, already
/// covered by `worktree_owner_cannot_terminal_transition_via_set_status`). No
/// `worktrees` row exists for the absent id, so no audit is — or could be —
/// stamped.
#[tokio::test]
async fn worktree_verdict_on_absent_worktree_is_not_found() {
    let pool = connect_in_memory().await.expect("pool");
    // An id that was never created via `create_worktree` — no row, no owner.
    let absent = "00000000-0000-0000-0000-000000000000";

    let merge = repo::record_worktree_merge(&pool, absent, Some("ref")).await;
    assert!(
        matches!(merge, Err(AppError::NotFound(_))),
        "merge on an absent worktree is NotFound (require_review_owner missing arm), got {merge:?}"
    );

    let reject = repo::record_worktree_rejection(&pool, absent, Some("nope")).await;
    assert!(
        matches!(reject, Err(AppError::NotFound(_))),
        "rejection on an absent worktree is NotFound (require_review_owner missing arm), got {reject:?}"
    );

    // No audit row exists for the absent id — neither call stamped anything.
    assert_eq!(
        worktree_outcome(&pool, absent).await,
        None,
        "a NotFound verdict stamps no audit (there is no worktree row at all)"
    );
}

// ===========================================================================
// 10. set_task_checkpoint NotFound branch (R22): a missing / soft-deleted task
//     (affected==0 under the `deleted_at IS NULL` guard) is a clean NotFound.
// ===========================================================================

/// `repo::set_task_checkpoint` on an ABSENT task id is `AppError::NotFound` (the
/// kind read finds no row), and on a SOFT-DELETED task it is likewise NotFound —
/// the UPDATE carries `AND deleted_at IS NULL`, so a tombstoned task yields
/// `affected==0` and the typed `work_item '{id}' not found`. (The non-`task`
/// Validation arm is covered by the repo's in-module
/// `set_task_checkpoint_roundtrip_event_and_scope`.)
#[tokio::test]
async fn set_task_checkpoint_on_missing_or_deleted_task_is_not_found() {
    let pool = connect_in_memory().await.expect("pool");

    // Absent id: no row at all ⇒ NotFound (the kind read fails first).
    let absent = repo::set_task_checkpoint(&pool, "no-such-task", true).await;
    assert!(
        matches!(absent, Err(AppError::NotFound(_))),
        "checkpoint on an absent task is NotFound, got {absent:?}"
    );

    // Soft-deleted task: the kind read succeeds but the UPDATE's `deleted_at IS
    // NULL` guard matches 0 rows ⇒ NotFound (affected==0 branch).
    let story = seed_chain_to_story(&pool).await;
    let task = repo::create_work_item(&pool, "task", Some(&story), "T", None)
        .await
        .expect("task")
        .to_string();
    sqlx::query("UPDATE work_items SET deleted_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(&task)
        .execute(&pool)
        .await
        .expect("soft-delete the task");

    let deleted = repo::set_task_checkpoint(&pool, &task, true).await;
    assert!(
        matches!(deleted, Err(AppError::NotFound(_))),
        "checkpoint on a soft-deleted task is NotFound (affected==0 under the deleted_at guard), got {deleted:?}"
    );
}

// ===========================================================================
// 11. Legal cancellation edges on a worktree-LESS sprint (R23): ready→cancelled,
//     active→cancelled, review→cancelled all succeed — the widened worktree-owner
//     terminal guard fires ONLY for worktree owners, so a worktree-less sprint
//     cancels freely.
// ===========================================================================

/// A sprint that owns NO worktree may cancel from every status the legal table
/// permits — `ready→cancelled`, `active→cancelled`, and `review→cancelled` — via
/// a bare `set_sprint_status`. This is the COMPLEMENT of
/// `worktree_owner_cannot_terminal_transition_via_set_status`: the migration-0016
/// terminal guard is scoped to worktree OWNERS, so a worktree-less sprint reaches
/// `cancelled` directly. Each edge is exercised on its own freshly-seeded sprint
/// driven to the required starting status along the legal path.
#[tokio::test]
async fn worktree_less_sprint_cancels_from_every_legal_edge() {
    let pool = connect_in_memory().await.expect("pool");

    // ready → cancelled.
    let ready_sprint = seed_sprint(&pool).await; // 'draft'
    repo::set_sprint_status(&pool, &ready_sprint, SprintStatus::Ready)
        .await
        .expect("draft → ready");
    repo::set_sprint_status(&pool, &ready_sprint, SprintStatus::Cancelled)
        .await
        .expect("ready → cancelled on a worktree-less sprint is legal");
    assert_eq!(sprint_status(&pool, &ready_sprint).await, "cancelled");

    // active → cancelled.
    let active_sprint = seed_sprint(&pool).await;
    activate_sprint(&pool, &active_sprint).await; // draft → ready → active
    repo::set_sprint_status(&pool, &active_sprint, SprintStatus::Cancelled)
        .await
        .expect("active → cancelled on a worktree-less sprint is legal");
    assert_eq!(sprint_status(&pool, &active_sprint).await, "cancelled");

    // review → cancelled.
    let review_sprint = seed_sprint(&pool).await;
    activate_sprint(&pool, &review_sprint).await;
    repo::set_sprint_status(&pool, &review_sprint, SprintStatus::Review)
        .await
        .expect("active → review");
    repo::set_sprint_status(&pool, &review_sprint, SprintStatus::Cancelled)
        .await
        .expect("review → cancelled on a worktree-less sprint is legal");
    assert_eq!(sprint_status(&pool, &review_sprint).await, "cancelled");
}

// ===========================================================================
// 12. record_task_commits partial idempotency (R24): a batch mixing one NEW pair
//     with one ALREADY-RECORDED pair returns inserted==1 (the new edge counts,
//     the duplicate collapses on the ON CONFLICT).
// ===========================================================================

/// `record_task_commits` returns the count of GENUINELY-new `(commit, task)`
/// edges. The repo's in-module coverage checks the full re-record → 0 case; this
/// asserts the PARTIAL/mixed case: a single batch carrying one already-recorded
/// pair AND one brand-new pair under the SAME commit sha collapses the duplicate
/// (ON CONFLICT) but inserts the new edge, so `inserted == 1`. Uses REAL task ids
/// (the post-fix `record_task_commits` validates task existence — a bogus id is
/// NotFound).
#[tokio::test]
async fn record_task_commits_partial_batch_counts_only_new_edges() {
    let pool = connect_in_memory().await.expect("pool");
    let story = seed_chain_to_story(&pool).await;
    let sprint = seed_sprint(&pool).await;
    let task_a = repo::create_work_item(&pool, "task", Some(&story), "TA", None)
        .await
        .expect("task a")
        .to_string();
    let task_b = repo::create_work_item(&pool, "task", Some(&story), "TB", None)
        .await
        .expect("task b")
        .to_string();

    // Pre-record (sha-1, task_a).
    let first = repo::record_task_commits(&pool, "sha-1", &[task_a.as_str()], Some(&sprint))
        .await
        .expect("first record");
    assert_eq!(first, 1, "the initial edge is inserted");

    // A mixed batch under the SAME sha: (sha-1, task_a) is a duplicate (collapses),
    // (sha-1, task_b) is brand new (inserts) ⇒ inserted == 1.
    let mixed = repo::record_task_commits(
        &pool,
        "sha-1",
        &[task_a.as_str(), task_b.as_str()],
        Some(&sprint),
    )
    .await
    .expect("mixed record");
    assert_eq!(
        mixed, 1,
        "the new (sha-1, task_b) edge counts; the duplicate (sha-1, task_a) collapses on ON CONFLICT"
    );
}

// ===========================================================================
// 13. Checkpoint-freeze negative discriminators (R25): the freeze predicate is
//     sprint-LOCAL and LIVE-only, so a soft-deleted checkpoint task (a) and an
//     in_progress checkpoint task in a DIFFERENT sprint (b) must NOT freeze THIS
//     sprint.
// ===========================================================================

/// (a) A SOFT-DELETED checkpoint task (`checkpoint=1` AND `deleted_at` set) does
/// NOT freeze its sprint. The freeze SELECT carries `c.deleted_at IS NULL`, so a
/// tombstoned checkpoint — even one left `in_progress` — is invisible to the
/// barrier, and `claim_next_task` still returns the ordinary ready task. This is
/// the live-only discriminator complementing the positive
/// `in_progress_checkpoint_freezes_then_resumes`.
#[tokio::test]
async fn soft_deleted_checkpoint_does_not_freeze_sprint() {
    let pool = connect_in_memory().await.expect("pool");
    let story = seed_chain_to_story(&pool).await;
    let sprint = seed_sprint(&pool).await;
    activate_sprint(&pool, &sprint).await;

    // A checkpoint task and an ordinary ready implement task in the sprint.
    let checkpoint = seed_queue_task(&pool, &story, &sprint, "CHECKPOINT").await;
    repo::set_task_checkpoint(&pool, &checkpoint, true)
        .await
        .expect("mark the task a checkpoint");
    let other = seed_queue_task(&pool, &story, &sprint, "OTHER").await;

    // Drive the checkpoint in_progress AND soft-delete it: a tombstoned checkpoint
    // is invisible to the `c.deleted_at IS NULL` freeze SELECT, so it must NOT
    // freeze the sprint (no sleep — a far-future lease keeps it in_progress).
    sqlx::query(
        "UPDATE work_items SET status = 'in_progress', assignee = 'agent-cp', \
         lease_expires_at = datetime('now', '+1800 seconds'), \
         deleted_at = CURRENT_TIMESTAMP WHERE id = $1",
    )
    .bind(&checkpoint)
    .execute(&pool)
    .await
    .expect("checkpoint in_progress + soft-deleted");

    // NOT frozen: the soft-deleted checkpoint is invisible to the barrier, so the
    // ordinary task is claimable.
    let claimed = repo::claim_next_task(&pool, &sprint, Lane::Implement, None, "agent-a", LEASE_TTL_SECS)
        .await
        .expect("claim runs")
        .expect("a soft-deleted checkpoint does not freeze the sprint");
    assert_eq!(
        claimed.task_id, other,
        "with the checkpoint soft-deleted, the ordinary task is claimed (no freeze)"
    );
}

/// (b) An `in_progress` checkpoint task in a DIFFERENT sprint must NOT freeze
/// THIS sprint. The freeze SELECT JOINs `sprint_tasks st ... WHERE st.sprint_id =
/// $1`, so the barrier is sprint-LOCAL — a checkpoint mid-flight in another
/// sprint is invisible here, and claims in this sprint proceed. This is the
/// sprint-scope discriminator complementing the positive freeze test.
#[tokio::test]
async fn checkpoint_in_other_sprint_does_not_freeze_this_sprint() {
    let pool = connect_in_memory().await.expect("pool");
    let story = seed_chain_to_story(&pool).await;

    // THIS sprint: one ordinary ready implement task, no checkpoint of its own.
    let this_sprint = seed_sprint(&pool).await;
    activate_sprint(&pool, &this_sprint).await;
    let mine = seed_queue_task(&pool, &story, &this_sprint, "MINE").await;

    // The OTHER sprint owns an in_progress checkpoint task — a sprint-wide barrier
    // for ITS sprint only.
    let other_sprint = seed_sprint(&pool).await;
    activate_sprint(&pool, &other_sprint).await;
    let other_checkpoint = seed_queue_task(&pool, &story, &other_sprint, "OTHER-CP").await;
    repo::set_task_checkpoint(&pool, &other_checkpoint, true)
        .await
        .expect("mark the other-sprint task a checkpoint");
    sqlx::query(
        "UPDATE work_items SET status = 'in_progress', assignee = 'agent-cp', \
         lease_expires_at = datetime('now', '+1800 seconds') WHERE id = $1",
    )
    .bind(&other_checkpoint)
    .execute(&pool)
    .await
    .expect("other-sprint checkpoint in_progress");

    // THIS sprint is NOT frozen by the OTHER sprint's checkpoint — the freeze JOIN
    // is keyed on `sprint_tasks.sprint_id = this_sprint`, so the barrier is local.
    let claimed = repo::claim_next_task(&pool, &this_sprint, Lane::Implement, None, "agent-a", LEASE_TTL_SECS)
        .await
        .expect("claim runs")
        .expect("a checkpoint in another sprint does not freeze this sprint");
    assert_eq!(
        claimed.task_id, mine,
        "this sprint's task is claimed — the other sprint's checkpoint does not freeze it"
    );
}
