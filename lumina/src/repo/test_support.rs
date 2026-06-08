//! Shared `#[cfg(test)]` fixtures for the `repo` unit tests.
//!
//! The seed-chain builders and the table-count / status helpers are used by
//! `#[test]`/`#[tokio::test]` functions across multiple domain clusters in
//! `repo/mod.rs`'s `mod tests`, so they live here as `pub(crate) fn`s. The test
//! module pulls them in with `use crate::repo::test_support::*;`. Every actual
//! test FUNCTION stays in `mod.rs` for now (carved with its cluster later).
//!
//! `use super::*` reaches the public repo mutators the seed builders drive
//! (`create_work_item`, `create_work_item_full`, `add_acceptance_criterion`,
//! `CreateOpts`); `SqlitePool` is a `#[cfg(test)]`-private import in `mod.rs`
//! and is re-declared here.

use sqlx::SqlitePool;

use super::*;
use crate::domain::NewSprint;

/// Row count of `work_items` (compile-checked literal — sqlx 0.9's
/// `SqlSafeStr` bound rejects a dynamically-built table name on the runtime
/// `query_as`, so the two count helpers are split per table).
pub(crate) async fn count_work_items(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM work_items")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Row count of `events`.
pub(crate) async fn count_events(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM events")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Count `events` rows for a given `aggregate_id` + `event_type` (used by the
/// R1 atomicity test to assert the two-event resolve shape).
pub(crate) async fn count_events_for(pool: &SqlitePool, aggregate_id: &str, event_type: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM events WHERE aggregate_id = $1 AND event_type = $2",
    )
    .bind(aggregate_id)
    .bind(event_type)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Build the legal project→epic→focus→story chain and return the story id,
/// so tests can create a legal `task` (or an illegal one) beneath it.
///
/// Migration-0010 valid-chain recipe: an epic must carry a non-empty outcome,
/// a focus must carry a shape, and a story can only be created once its
/// ancestor epic has ≥1 close-criterion. The chain therefore writes 4
/// work_items (project/epic/focus/story) + 5 events (the four creates plus the
/// epic close-criterion add).
pub(crate) async fn seed_chain_to_story(pool: &SqlitePool) -> String {
    let project = create_work_item(pool, "project", None, "P", None)
        .await
        .expect("legal project");
    let epic = create_work_item_full(
        pool,
        "epic",
        Some(&project.to_string()),
        "E",
        None,
        CreateOpts {
            origin: None,
            outcome: Some("the epic outcome"),
            shape: None,
            lane: None,
        },
    )
    .await
    .expect("legal epic");
    add_acceptance_criterion(pool, &epic.to_string(), "epic close criterion")
        .await
        .expect("epic close criterion");
    let focus = create_work_item_full(
        pool,
        "focus",
        Some(&epic.to_string()),
        "FO",
        None,
        CreateOpts {
            origin: None,
            outcome: None,
            shape: Some("vertical-slice"),
            lane: None,
        },
    )
    .await
    .expect("legal focus");
    let story = create_work_item(pool, "story", Some(&focus.to_string()), "S", None)
        .await
        .expect("legal story");
    story.to_string()
}

/// Build the legal project→epic→focus chain and return the FOCUS id. Used by
/// the SpawnStory test (R6): a `story` child needs a `focus` parent, so a
/// SpawnStory decision is only reachable when the finding hosts directly on a
/// focus. The epic carries ≥1 close-criterion so a story create under the
/// focus passes the close-criterion gate.
pub(crate) async fn seed_chain_to_focus(pool: &SqlitePool) -> String {
    let project = create_work_item(pool, "project", None, "P", None)
        .await
        .expect("legal project");
    let epic = create_work_item_full(
        pool,
        "epic",
        Some(&project.to_string()),
        "E",
        None,
        CreateOpts {
            origin: None,
            outcome: Some("the epic outcome"),
            shape: None,
            lane: None,
        },
    )
    .await
    .expect("legal epic");
    add_acceptance_criterion(pool, &epic.to_string(), "epic close criterion")
        .await
        .expect("epic close criterion");
    let focus = create_work_item_full(
        pool,
        "focus",
        Some(&epic.to_string()),
        "FO",
        None,
        CreateOpts {
            origin: None,
            outcome: None,
            shape: Some("vertical-slice"),
            lane: None,
        },
    )
    .await
    .expect("legal focus");
    focus.to_string()
}

/// Row count of `work_item_activity`.
pub(crate) async fn count_activity(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM work_item_activity")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Row count of `acceptance_criteria`.
pub(crate) async fn count_criteria(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM acceptance_criteria")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Read a single work item's status (test helper).
pub(crate) async fn item_status(pool: &SqlitePool, id: &str) -> String {
    sqlx::query_scalar::<_, String>("SELECT status FROM work_items WHERE id = ?1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Count events of a given `event_type` (test helper — proves the
/// exactly-one-event-per-logical-write invariant for the multi-write resolve).
pub(crate) async fn count_events_of_type(pool: &SqlitePool, event_type: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM events WHERE event_type = ?1")
        .bind(event_type)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Seed a legal sprint with no tasks; returns the sprint id. Shared by the
/// runs/sprints cluster tests (`runs_sprints.rs`) and the team-execution
/// claim/complete tests (`team_execution.rs`) + the readiness/quiescence tests
/// (`readiness.rs`).
pub(crate) async fn seed_sprint(pool: &SqlitePool) -> String {
    create_sprint(
        pool,
        &NewSprint {
            title: Some("S1".into()),
            worktree_id: None,
            predecessor_sprint_id: None,
        },
    )
        .await
        .expect("legal sprint")
        .to_string()
}

/// Create a `task` under `story`, stamp its `lane` (and optional `tier`),
/// and bind it to `sprint`. Returns the task id. `tier` is a wire-form
/// string (`"lite"`/`"deep"`) or `None`. Shared by the team-execution claim/
/// release/complete tests (`team_execution.rs`) and the quiescence/open-question
/// readiness tests (`readiness.rs`).
pub(crate) async fn seed_queue_task(
    pool: &SqlitePool,
    story: &str,
    sprint: &str,
    title: &str,
    lane: Option<&str>,
    tier: Option<&str>,
) -> String {
    let task = create_work_item(pool, "task", Some(story), title, None)
        .await
        .expect("task")
        .to_string();
    // Stamp lane + tier directly (no repo mutator for `lane` yet) and move
    // the task to the queue-ready `todo` status. `create_work_item` stamps
    // the literal `status="open"` (the create default); the claim's
    // readiness set is `{todo, open}` (both are "ready, not started"), so a
    // task staged at `todo` by the planning flow is claimable — this helper
    // exercises that path. The `'open'`-preserving path (the create default,
    // covering spawned review/rework tasks) is exercised by
    // `seed_queue_task_open` + `claim_returns_open_status_task`.
    sqlx::query("UPDATE work_items SET lane = $2, tier = $3, status = 'todo' WHERE id = $1")
        .bind(&task)
        .bind(lane)
        .bind(tier)
        .execute(pool)
        .await
        .expect("stamp lane/tier/status");
    add_tasks_to_sprint(pool, sprint, &[task.as_str()])
        .await
        .expect("bind task to sprint");
    task
}

/// Like [`seed_queue_task`] but PRESERVES the `create_work_item` default
/// `status='open'` (stamps only `lane`/`tier`, never touches `status`). This
/// is the real-world shape of a freshly-created task — and specifically of
/// the review task `complete_task` (T6) and the rework task
/// `record_finding_decision` (T8) spawn via the create path. A claim that
/// keyed on `status='todo'` only would never see these.
pub(crate) async fn seed_queue_task_open(
    pool: &SqlitePool,
    story: &str,
    sprint: &str,
    title: &str,
    lane: Option<&str>,
    tier: Option<&str>,
) -> String {
    let task = create_work_item(pool, "task", Some(story), title, None)
        .await
        .expect("task")
        .to_string();
    sqlx::query("UPDATE work_items SET lane = $2, tier = $3 WHERE id = $1")
        .bind(&task)
        .bind(lane)
        .bind(tier)
        .execute(pool)
        .await
        .expect("stamp lane/tier (status left at create-default 'open')");
    add_tasks_to_sprint(pool, sprint, &[task.as_str()])
        .await
        .expect("bind task to sprint");
    task
}

/// Read a task's (status, assignee, lease_expires_at) for assertions. Shared by
/// the team-execution claim/release/complete tests (`team_execution.rs`).
pub(crate) async fn task_lease_state(
    pool: &SqlitePool,
    task_id: &str,
) -> (String, Option<String>, Option<String>) {
    use sqlx::Row as _;
    let r = sqlx::query(
        "SELECT status, assignee, lease_expires_at FROM work_items WHERE id = $1",
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
    .expect("task row");
    (
        r.try_get("status").unwrap(),
        r.try_get("assignee").unwrap(),
        r.try_get("lease_expires_at").unwrap(),
    )
}
