//! `get_checkpoint_suggestions` repo-logic acceptance test (1B-F8, T3).
//!
//! Exercises the cross-task EXPECTED files-overlap → checkpoint-candidate
//! computation behind `repo::story_checkpoint_suggestions` /
//! `repo::sprint_checkpoint_suggestions` (built on the first-class `task_files`
//! EXPECTED set via the private `task_expected_overlap_keys` helper). The two
//! scopes share one core (`checkpoint_suggestions_over`), so both wrappers are
//! covered, plus the omission rules (no-overlap omitted, empty-EXPECTED omitted)
//! and the multi-peer overlap list.
//!
//! All seeding goes through the public `repo::*` path (the same fns the MCP
//! `set_task_spec` / `create_sprint` / `add_tasks_to_sprint` tools wrap), and
//! `connect_in_memory` runs every embedded migration (including 0023, which
//! created `task_files`), so this introduces no `.sqlx` cache entry — the
//! runtime `sqlx::query*` discipline.

use lumina_core::db::connect_in_memory;
use lumina_core::repo::{self, CheckpointSuggestion};
use serde_json::json;
use sqlx::SqlitePool;

/// Seed a legal `project → epic → focus → story` chain and return the story id.
/// Mirrors the sibling `http/files.rs` seed_chain (the epic carries the
/// migration-0010-mandatory `outcome`; the focus the mandatory `shape`).
async fn seed_story(pool: &SqlitePool) -> String {
    let project = repo::create_work_item(pool, "project", None, "P", None)
        .await
        .expect("project")
        .to_string();
    let epic = repo::create_work_item_full(
        pool,
        "epic",
        Some(&project),
        "E",
        None,
        repo::CreateOpts { origin: None, outcome: Some("the epic outcome"), shape: None, lane: None },
    )
    .await
    .expect("epic")
    .to_string();
    // The story-create gate requires the ancestor epic to carry ≥1 close-criterion.
    repo::add_acceptance_criterion(pool, &epic, "epic close criterion")
        .await
        .expect("epic close criterion");
    let focus = repo::create_work_item_full(
        pool,
        "focus",
        Some(&epic),
        "FO",
        None,
        repo::CreateOpts { origin: None, outcome: None, shape: Some("vertical-slice"), lane: None },
    )
    .await
    .expect("focus")
    .to_string();
    repo::create_work_item(pool, "story", Some(&focus), "S", None)
        .await
        .expect("story")
        .to_string()
}

/// Create one `task` under `story` and return its id.
async fn task(pool: &SqlitePool, story: &str, title: &str) -> String {
    repo::create_work_item(pool, "task", Some(story), title, None)
        .await
        .expect("task")
        .to_string()
}

/// Open a sprint and return its id.
async fn seed_sprint(pool: &SqlitePool) -> String {
    repo::create_sprint(
        pool,
        &lumina_core::domain::NewSprint {
            title: None,
            worktree_id: None,
            predecessor_sprint_id: None,
        },
    )
    .await
    .expect("sprint")
    .to_string()
}

/// Borrow the suggestion for `task_id`, or panic — the membership the assertions
/// key off.
fn suggestion_for<'a>(
    suggestions: &'a [CheckpointSuggestion],
    task_id: &str,
) -> &'a CheckpointSuggestion {
    suggestions
        .iter()
        .find(|s| s.task_id == task_id)
        .unwrap_or_else(|| panic!("expected a checkpoint suggestion for {task_id}: {suggestions:?}"))
}

/// The candidate task ids present in a suggestion list (order-independent).
fn candidate_ids(suggestions: &[CheckpointSuggestion]) -> std::collections::BTreeSet<&str> {
    suggestions.iter().map(|s| s.task_id.as_str()).collect()
}

/// The peer task ids a candidate overlaps with (order-independent).
fn peer_ids(s: &CheckpointSuggestion) -> std::collections::BTreeSet<&str> {
    s.overlaps.iter().map(|o| o.task_id.as_str()).collect()
}

/// Two tasks expecting a shared file are BOTH surfaced as checkpoint candidates
/// (each pointing at the other on the shared path); a task overlapping nothing,
/// and a task with NO expected set, are OMITTED. The story scope and the sprint
/// scope (binding the same tasks via `sprint_tasks`) return the same verdict.
#[tokio::test]
async fn overlap_surfaces_candidates_and_omits_non_overlap_and_empty() {
    let pool = connect_in_memory().await.expect("pool");
    let story = seed_story(&pool).await;

    let t1 = task(&pool, &story, "T1").await;
    let t2 = task(&pool, &story, "T2").await;
    let t3 = task(&pool, &story, "T3").await;
    let t4 = task(&pool, &story, "T4").await;

    // t1 ∩ t2 = {src/shared.rs}; t3 disjoint; t4 has NO expected set.
    repo::set_task_expected_files(&pool, &t1, &[json!("src/a.rs"), json!("src/shared.rs")])
        .await
        .expect("t1 expected");
    repo::set_task_expected_files(&pool, &t2, &[json!("src/shared.rs"), json!("src/b.rs")])
        .await
        .expect("t2 expected");
    repo::set_task_expected_files(&pool, &t3, &[json!("src/c.rs")])
        .await
        .expect("t3 expected");
    // t4: deliberately no set_task_expected_files call → empty EXPECTED keys.

    // --- Story scope ---
    let story_suggestions = repo::story_checkpoint_suggestions(&pool, &story)
        .await
        .expect("story checkpoint suggestions");
    assert_eq!(
        candidate_ids(&story_suggestions),
        [t1.as_str(), t2.as_str()].into_iter().collect(),
        "only the two tasks sharing src/shared.rs are candidates (t3 disjoint, t4 empty omitted)"
    );
    let s1 = suggestion_for(&story_suggestions, &t1);
    assert_eq!(peer_ids(s1), [t2.as_str()].into_iter().collect(), "t1 overlaps t2");
    assert_eq!(
        s1.overlaps[0].shared_paths,
        vec!["src/shared.rs".to_owned()],
        "t1↔t2 share exactly src/shared.rs"
    );
    let s2 = suggestion_for(&story_suggestions, &t2);
    assert_eq!(peer_ids(s2), [t1.as_str()].into_iter().collect(), "t2 overlaps t1");
    assert_eq!(s2.overlaps[0].shared_paths, vec!["src/shared.rs".to_owned()]);

    // --- Sprint scope: bind all four tasks; same verdict over sprint_tasks. ---
    let sprint = seed_sprint(&pool).await;
    repo::add_tasks_to_sprint(
        &pool,
        &sprint,
        &[t1.as_str(), t2.as_str(), t3.as_str(), t4.as_str()],
    )
    .await
    .expect("bind tasks to sprint");

    let sprint_suggestions = repo::sprint_checkpoint_suggestions(&pool, &sprint)
        .await
        .expect("sprint checkpoint suggestions");
    assert_eq!(
        candidate_ids(&sprint_suggestions),
        [t1.as_str(), t2.as_str()].into_iter().collect(),
        "sprint scope surfaces the same candidates as the story scope"
    );
}

/// A path expected by THREE tasks makes each a candidate whose `overlaps` lists
/// BOTH of the other two — the multi-peer overlap path.
#[tokio::test]
async fn three_way_overlap_lists_both_peers() {
    let pool = connect_in_memory().await.expect("pool");
    let story = seed_story(&pool).await;
    let t1 = task(&pool, &story, "T1").await;
    let t2 = task(&pool, &story, "T2").await;
    let t3 = task(&pool, &story, "T3").await;

    for t in [&t1, &t2, &t3] {
        repo::set_task_expected_files(&pool, t, &[json!("src/hot.rs")])
            .await
            .expect("expected hot.rs");
    }

    let suggestions = repo::story_checkpoint_suggestions(&pool, &story)
        .await
        .expect("suggestions");
    assert_eq!(suggestions.len(), 3, "all three tasks are candidates");
    let s1 = suggestion_for(&suggestions, &t1);
    assert_eq!(
        peer_ids(s1),
        [t2.as_str(), t3.as_str()].into_iter().collect(),
        "t1 overlaps both t2 and t3 on the shared hot path"
    );
    for o in &s1.overlaps {
        assert_eq!(o.shared_paths, vec!["src/hot.rs".to_owned()]);
    }
}

/// An unknown / childless story and an unknown / empty sprint both yield an empty
/// list — no overlap, never an error.
#[tokio::test]
async fn unknown_scope_is_empty() {
    let pool = connect_in_memory().await.expect("pool");
    assert!(
        repo::story_checkpoint_suggestions(&pool, "does-not-exist")
            .await
            .expect("unknown story is Ok")
            .is_empty(),
        "an unknown story yields no candidates"
    );
    assert!(
        repo::sprint_checkpoint_suggestions(&pool, "does-not-exist")
            .await
            .expect("unknown sprint is Ok")
            .is_empty(),
        "an unknown sprint yields no candidates"
    );
}
