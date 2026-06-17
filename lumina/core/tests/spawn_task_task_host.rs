//! AC9 regression (1B-F9 MF): `record_finding_decision(spawn_task)` on a
//! TASK-hosted review finding must spawn a rework task that nests under the
//! host task's parent STORY and binds to the sprint — so sprint quiescence
//! BLOCKS on the rework (the answered "rework blocks completion" decision,
//! Q 019ebc7c-5fda).
//!
//! ## Why a dedicated test file
//! Kept OUT of `server/tests/e2e.rs` (which the E2E-rewrite task #3 owns) to
//! avoid same-file contention. This is a `lumina-core` integration test, so it
//! drives only the PUBLIC `repo::*` API — the in-crate `#[cfg(test)]` seed
//! helpers (`seed_chain_to_story` etc.) are invisible here, so the legal
//! `project → epic → focus → story` chain is reconstructed from public calls
//! (the same idiom as `tests/claim_concurrency.rs`).
//!
//! ## The disputed failure mode this pins down
//! BEFORE the MF fix, a `spawn_task` on a TASK-hosted finding hard-fails: the
//! spawn parents the new `task` directly under the finding's host, and a
//! task-under-task edge is rejected by `validate_hierarchy_edge` ("a 'task'
//! must sit under a 'story', not under a 'task'"), rolling back the WHOLE
//! `record_finding_decision`. (Even past that gate the sprint-binding fallback
//! `WHERE t.parent_id = host` would find nothing for a task host, leaving the
//! rework sprint-UNBOUND.) The MF fix resolves a task host UP to its parent
//! story so the rework nests under the story AND inherits the story's sprint.

use lumina_core::db;
use lumina_core::domain::{FindingDecisionKind, NewFindingDecision, NewSprint};
use lumina_core::repo::{self, CreateOpts, NewFinding};
use sqlx::SqlitePool;

/// Build the legal `project → epic → focus → story` chain via the PUBLIC repo
/// API and return the story id (mirrors `tests/claim_concurrency.rs`).
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
        CreateOpts { origin: None, outcome: Some("the epic outcome"), shape: None, lane: None },
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
        CreateOpts { origin: None, outcome: None, shape: Some("vertical-slice"), lane: None },
    )
    .await
    .expect("legal focus");
    let story = repo::create_work_item(pool, "story", Some(&focus.to_string()), "S", None)
        .await
        .expect("legal story");
    story.to_string()
}

/// Open a sprint and force it `active` (the migration-0016 claim/quiescence
/// guard runs only against `'active'`). Raw runtime sqlx for the status set —
/// NOT a compile-time macro, so the macro-eradication gate stays at 0.
async fn seed_active_sprint(pool: &SqlitePool) -> String {
    let sprint = repo::create_sprint(
        pool,
        &NewSprint { title: Some("S1".into()), worktree_id: None, predecessor_sprint_id: None },
    )
    .await
    .expect("legal sprint")
    .to_string();
    sqlx::query("UPDATE sprints SET status = 'active' WHERE id = $1")
        .bind(&sprint)
        .execute(pool)
        .await
        .expect("activate sprint");
    sprint
}

/// Read a work item's (kind, parent_id). Raw runtime sqlx read — not a macro.
async fn kind_and_parent(pool: &SqlitePool, id: &str) -> (String, Option<String>) {
    use sqlx::Row as _;
    let r = sqlx::query("SELECT kind, parent_id FROM work_items WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("work_item row");
    (r.try_get("kind").unwrap(), r.try_get("parent_id").unwrap())
}

/// AC9: a TASK-hosted review finding → `spawn_task` → the rework task nests
/// under the host task's parent STORY, is `lane='implement'`, and is BOUND to
/// the sprint so `get_sprint_quiescence` BLOCKS on it (claimable, not done).
#[tokio::test]
async fn spawn_task_on_task_hosted_finding_nests_under_story_and_binds_sprint() {
    let pool = db::connect_in_memory().await.expect("pool");
    let story = seed_chain_to_story(&pool).await;
    let sprint = seed_active_sprint(&pool).await;

    // An IMPLEMENT task under the story, bound to the sprint and driven to
    // terminal `done` (the impl task that produced the review finding).
    let impl_task = repo::create_work_item(&pool, "task", Some(&story), "IMPL", None)
        .await
        .expect("impl task")
        .to_string();
    repo::add_tasks_to_sprint(&pool, &sprint, &[impl_task.as_str()])
        .await
        .expect("bind impl task to sprint");
    repo::update_work_item_status(&pool, &impl_task, "done")
        .await
        .expect("impl task → done");

    // A review finding HOSTED ON THE TASK (the disputed host shape).
    let finding = repo::create_finding(
        &pool,
        &impl_task,
        &NewFinding { summary: Some("rework: fix the off-by-one"), ..NewFinding::default() },
    )
    .await
    .expect("task-hosted finding")
    .to_string();

    // Quiescence BEFORE the spawn: the impl task is terminal, so an all-terminal
    // sprint reads done=true. The spawn must flip this to NOT done.
    let before = repo::get_sprint_quiescence(&pool, &sprint)
        .await
        .expect("quiescence before spawn");
    assert!(before.done, "pre-spawn: the only task is done → sprint is done");

    // The decision under test: spawn_task on the TASK-hosted finding. PRE-FIX
    // this returns Err(Validation) (task-under-task), rolling the decision back.
    let (_decision_id, spawned) = repo::record_finding_decision(
        &pool,
        &NewFindingDecision {
            finding_id: finding.clone(),
            decision: FindingDecisionKind::SpawnTask,
            decided_by: Some("reviewer".into()),
        },
    )
    .await
    .expect("spawn_task on a task-hosted finding must succeed (MF fix)");

    let rework = spawned.expect("spawn_task yields a rework work_item id").to_string();

    // The rework task nests under the host task's parent STORY (NOT the task).
    let (kind, parent) = kind_and_parent(&pool, &rework).await;
    assert_eq!(kind, "task", "the rework is a task");
    assert_eq!(
        parent.as_deref(),
        Some(story.as_str()),
        "the rework task nests under the host task's parent STORY, not the task"
    );

    // It is lane='implement' (re-enters the implement claim queue) and bound to
    // the sprint (so the §C claim JOIN + quiescence can see it).
    use sqlx::Row as _;
    let row = sqlx::query("SELECT lane FROM work_items WHERE id = $1")
        .bind(&rework)
        .fetch_one(&pool)
        .await
        .expect("rework row");
    let lane: Option<String> = row.try_get("lane").unwrap();
    assert_eq!(lane.as_deref(), Some("implement"), "rework re-enters the implement lane");

    let bound = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sprint_tasks WHERE sprint_id = $1 AND task_id = $2",
    )
    .bind(&sprint)
    .bind(&rework)
    .fetch_one(&pool)
    .await
    .expect("count sprint binding");
    assert_eq!(bound, 1, "the rework task is bound to the sprint (inherited via the story)");

    // Quiescence AFTER the spawn: the sprint-bound rework is claimable, so the
    // sprint is NO LONGER done — quiescence BLOCKS on the rework (AC9 / the
    // answered "rework blocks completion" decision).
    let after = repo::get_sprint_quiescence(&pool, &sprint)
        .await
        .expect("quiescence after spawn");
    assert_eq!(after.claimable, 1, "the sprint-bound rework task is claimable");
    assert!(
        !after.done,
        "AC9: the sprint-bound rework keeps the sprint NOT done — quiescence blocks on it"
    );
}

/// No-regression: the STORY-hosted `spawn_task` path still works — the rework
/// nests directly under the story host and binds to the sprint exactly as
/// before the task-host resolution was added.
#[tokio::test]
async fn spawn_task_on_story_hosted_finding_still_nests_under_story() {
    let pool = db::connect_in_memory().await.expect("pool");
    let story = seed_chain_to_story(&pool).await;
    let sprint = seed_active_sprint(&pool).await;

    // A pre-existing task binds the STORY to the sprint (so the story-membership
    // binding fallback can resolve the sprint for the rework).
    let anchor = repo::create_work_item(&pool, "task", Some(&story), "ANCHOR", None)
        .await
        .expect("anchor task")
        .to_string();
    repo::add_tasks_to_sprint(&pool, &sprint, &[anchor.as_str()])
        .await
        .expect("bind anchor to sprint");

    // A review finding hosted on the STORY (the legacy path).
    let finding = repo::create_finding(
        &pool,
        &story,
        &NewFinding { summary: Some("story-level follow-up"), ..NewFinding::default() },
    )
    .await
    .expect("story-hosted finding")
    .to_string();

    let (_decision_id, spawned) = repo::record_finding_decision(
        &pool,
        &NewFindingDecision {
            finding_id: finding.clone(),
            decision: FindingDecisionKind::SpawnTask,
            decided_by: Some("reviewer".into()),
        },
    )
    .await
    .expect("spawn_task on a story-hosted finding");

    let rework = spawned.expect("spawn_task yields a rework id").to_string();
    let (kind, parent) = kind_and_parent(&pool, &rework).await;
    assert_eq!(kind, "task", "the rework is a task");
    assert_eq!(
        parent.as_deref(),
        Some(story.as_str()),
        "story-hosted spawn still nests directly under the story (no regression)"
    );

    let bound = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sprint_tasks WHERE sprint_id = $1 AND task_id = $2",
    )
    .bind(&sprint)
    .bind(&rework)
    .fetch_one(&pool)
    .await
    .expect("count sprint binding");
    assert_eq!(bound, 1, "the story-hosted rework still binds to the sprint via the story");
}
