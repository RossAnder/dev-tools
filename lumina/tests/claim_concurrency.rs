//! Claim-concurrency correctness gate for the team-execution work queue
//! (plan T11 of `docs/plans/eventual-leaping-metcalfe.md`, the plan's stated
//! CORE RISK).
//!
//! `repo::claim_next_task` is the race-sensitive primitive: one
//! `BEGIN IMMEDIATE` SELECT→UPDATE transaction leases the next ready task to a
//! single agent. The property under test is that **no two agents ever claim the
//! same task** under contention, and that the writer-serialising config (WAL +
//! 5s `busy_timeout`, set by `db::init` for an on-disk DB) absorbs the burst
//! WITHOUT surfacing `SQLITE_BUSY`. This is the property the agent-teams shared
//! task list cannot give and lumina's single-writer txn does.
//!
//! Harness shape is copied from `tests/concurrency.rs`: an on-disk
//! `tempfile::TempDir` database (an in-memory DB has no real shared-cache lock
//! manager in default mode, so it cannot exercise this path), N concurrent
//! tasks on a multi-thread runtime, joined, with every claim's
//! `Result` unwrapped so a stray `SQLITE_BUSY` (or any other error) FAILS the
//! test rather than being silently swallowed.
//!
//! ## Determinism — no sleeps, no TTL elapsing
//!
//! The crate forbids flaky time-based tests. The lazy-reclaim leg therefore
//! does NOT wait for a lease to expire: it SEEDS a row whose `lease_expires_at`
//! is a literal PAST timestamp (`'2000-01-01 00:00:00'`, the same idiom the
//! repo-layer claim unit test `claim_lazily_reclaims_expired_lease_*` uses) and
//! asserts the very next claim reclaims it. No `tokio::time::sleep`, no TTL
//! countdown — the past timestamp is already "expired" the instant the test
//! starts.

use std::collections::HashMap;
use std::sync::Arc;

use lumina::db;
use lumina::domain::{ClaimedTask, Lane};
use lumina::repo::{self, CreateOpts};
use sqlx::SqlitePool;
use tokio::task::JoinSet;

/// N concurrent agents contending on the queue (matches `tests/concurrency.rs`).
const CONCURRENT_AGENTS: usize = 8;
/// M ready implement-lane tasks seeded into the sprint. With M < N the queue
/// drains to exactly M successful claims and (N - M) agents come up empty —
/// proving both "no double-claim" AND "claims == min(N, M)".
const READY_TASKS: usize = 4;
/// Generous lease TTL (seconds) — the contention test never relies on a lease
/// expiring; this just stamps a far-future deadline so a claimed task stays
/// leased for the test's duration.
const LEASE_TTL_SECS: i64 = 1800;

/// Open the on-disk SQLite pool used by the concurrency tests. WAL + the 5s
/// busy_timeout are enabled by `db::init` (the `is_in_memory` gate evaluates to
/// false for a tempdir path) — those are exactly what serialise the writers, so
/// they must NOT be weakened. Mirrors `tests/concurrency.rs::open_on_disk_pool`.
async fn open_on_disk_pool() -> (tempfile::TempDir, SqlitePool) {
    let tmp = tempfile::tempdir().expect("create tempdir for on-disk SQLite pool");
    let db_path = tmp.path().join("claim_concurrency.db");
    let url = db_path.to_string_lossy().into_owned();
    let pool = db::init(&url).await.expect("init on-disk pool");
    (tmp, pool)
}

/// Build the legal `project → epic → focus → story` chain via the PUBLIC repo
/// API and return the story id. The repo's own `seed_chain_to_story` is a
/// `#[cfg(test)]`-private helper invisible to an integration test, so we
/// reconstruct it here with the same public calls (`create_work_item_full` +
/// `CreateOpts` for the mandatory epic `outcome` / focus `shape`, and an epic
/// close-criterion so a story create under the focus passes the gate).
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

/// Create a `task` under `story`, stamp its `lane` (and optional `tier`), move
/// it to a queue-ready `status='todo'`, and bind it to `sprint`. Returns the
/// task id.
///
/// `lane`/`status`/`lease_expires_at` have NO public repo mutator (those land
/// with the lease lifecycle, plan T5/T6), so — exactly like the repo-layer
/// claim unit tests' `seed_queue_task` helper — we stamp them with a raw
/// runtime `sqlx::query` UPDATE. Raw runtime sqlx is permitted for seeding (it
/// is NOT a `sqlx::query!` compile-time macro, so the macro-eradication gate
/// stays at 0).
async fn seed_queue_task(
    pool: &SqlitePool,
    story: &str,
    sprint: &str,
    title: &str,
    lane: &str,
    tier: Option<&str>,
) -> String {
    let task = repo::create_work_item(pool, "task", Some(story), title, None)
        .await
        .expect("legal task")
        .to_string();
    // Stamp lane + tier and move to the queue-ready `todo` status (the claim's
    // readiness set is {todo, open}; `create_work_item` defaults to 'open').
    sqlx::query("UPDATE work_items SET lane = $2, tier = $3, status = 'todo' WHERE id = $1")
        .bind(&task)
        .bind(lane)
        .bind(tier)
        .execute(pool)
        .await
        .expect("stamp lane/tier/status");
    repo::add_tasks_to_sprint(pool, sprint, &[task.as_str()])
        .await
        .expect("bind task to sprint");
    task
}

/// Open a sprint via the public repo API; returns its id.
async fn seed_sprint(pool: &SqlitePool) -> String {
    repo::create_sprint(pool, &lumina::domain::NewSprint { title: Some("S1".into()) })
        .await
        .expect("legal sprint")
        .to_string()
}

/// **The correctness gate.** N=8 agents (distinct ids) concurrently drain a
/// sprint of M=4 ready implement-lane tasks, each looping `claim_next_task`
/// until it returns `None`. Joined, we assert:
///
/// * NO error surfaced from any claim — in particular no `SQLITE_BUSY` (the WAL
///   + 5s busy_timeout serialise the writers; a regression that weakened them
///   would surface here as an `Err`).
/// * Each task is claimed by AT MOST ONE agent (no two agents share a task_id),
///   and the recorded `assignee` matches the claiming agent.
/// * Total successful claims == `min(N, M)` == M (every ready task is claimed
///   exactly once; the surplus agents come up empty).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_claims_never_double_claim() {
    let (_tmp, pool) = open_on_disk_pool().await;

    // Seed the chain + sprint + M ready implement-lane tasks BEFORE spawning the
    // contending agents (seeding is sequential; only the claims race).
    let story = seed_chain_to_story(&pool).await;
    let sprint = seed_sprint(&pool).await;
    let mut seeded_ids = Vec::with_capacity(READY_TASKS);
    for i in 0..READY_TASKS {
        let id = seed_queue_task(
            &pool,
            &story,
            &sprint,
            &format!("Ready Task {i}"),
            "implement",
            Some("deep"),
        )
        .await;
        seeded_ids.push(id);
    }

    let pool = Arc::new(pool);
    let sprint = Arc::new(sprint);

    // Each agent loops: claim until the queue is drained (`Ok(None)`). It returns
    // the full list of tasks it managed to claim. An `Err` (e.g. SQLITE_BUSY)
    // propagates out and fails the join below.
    let mut agents = JoinSet::new();
    for a in 0..CONCURRENT_AGENTS {
        let pool = Arc::clone(&pool);
        let sprint = Arc::clone(&sprint);
        let agent_id = format!("agent-{a}");
        agents.spawn(async move {
            let mut mine: Vec<ClaimedTask> = Vec::new();
            loop {
                // `&*pool` is `&SqlitePool`, which impls `DbClient` — the same
                // deref the sibling `tests/concurrency.rs` uses (trait-bound
                // resolution does not auto-deref the `Arc`).
                let claimed = repo::claim_next_task(
                    &*pool,
                    &sprint,
                    Lane::Implement,
                    None,
                    &agent_id,
                    LEASE_TTL_SECS,
                )
                .await?;
                match claimed {
                    Some(task) => mine.push(task),
                    None => break, // queue drained for this agent
                }
            }
            Ok::<(String, Vec<ClaimedTask>), lumina::error::AppError>((agent_id, mine))
        });
    }

    // Join every agent. `result.expect(...)` unwraps the `Result<_, AppError>`:
    // a SQLITE_BUSY (or any DB error) escaping a claim fails the test HERE.
    let mut claims_by_task: HashMap<String, Vec<String>> = HashMap::new();
    let mut total_claims = 0usize;
    while let Some(joined) = agents.join_next().await {
        let (agent_id, claimed) = joined
            .expect("agent task panicked")
            .unwrap_or_else(|e| panic!("claim_next_task errored under contention (no SQLITE_BUSY expected): {e}"));
        for task in claimed {
            // The claim must stamp the claiming agent as the assignee.
            assert_eq!(
                task.assignee, agent_id,
                "a claimed task's assignee must be the claiming agent"
            );
            assert!(
                !task.lease_expires_at.is_empty(),
                "a claimed task must carry a stamped lease deadline"
            );
            claims_by_task
                .entry(task.task_id.clone())
                .or_default()
                .push(agent_id.clone());
            total_claims += 1;
        }
    }

    // No double-claim: every task id was claimed by exactly one agent.
    for (task_id, claimers) in &claims_by_task {
        assert_eq!(
            claimers.len(),
            1,
            "task {task_id} was double-claimed by {claimers:?} — the SELECT→UPDATE \
             txn failed to serialise"
        );
    }

    // Every seeded ready task was claimed exactly once.
    for id in &seeded_ids {
        assert!(
            claims_by_task.contains_key(id),
            "ready task {id} was never claimed — the queue did not fully drain"
        );
    }

    // Total successful claims == min(N, M) == M. With N (8) > M (4), the surplus
    // agents come up empty and the queue drains to exactly M unique claims.
    let expected = READY_TASKS.min(CONCURRENT_AGENTS);
    assert_eq!(
        total_claims, expected,
        "expected exactly {expected} successful claims (min(agents, ready tasks)), got {total_claims}"
    );
    assert_eq!(
        claims_by_task.len(),
        expected,
        "expected {expected} distinct claimed tasks, got {}",
        claims_by_task.len()
    );

    // Belt-and-braces against the DB: exactly M rows are now leased in_progress
    // with a non-null assignee, and none is left unleased. (Raw runtime sqlx
    // read — not a macro.)
    //
    // NOTE we do NOT assert "one distinct assignee per task": the no-double-claim
    // property is "each TASK has exactly one claimer" (asserted above via
    // `claims_by_task[task].len() == 1`), NOT "each task has a different
    // claimer". A fast agent can legitimately win several tasks in its drain loop
    // before slower agents wake, so M tasks may be spread across FEWER than M
    // agents — that is correct contention behaviour, not a race.
    let in_progress: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM work_items \
         WHERE status = 'in_progress' AND assignee IS NOT NULL \
           AND id IN (SELECT task_id FROM sprint_tasks WHERE sprint_id = $1)",
    )
    .bind(sprint.as_str())
    .fetch_one(&*pool)
    .await
    .expect("count in_progress leased tasks");
    assert_eq!(
        in_progress, expected as i64,
        "exactly {expected} sprint tasks should be leased in_progress after the drain"
    );
}

/// **Lazy-reclaim determinism (no sleep).** A task seeded `in_progress` with a
/// literal PAST `lease_expires_at` (owned by a now-dead agent) is reclaimed by
/// the very next claim and re-leased to the live claimer — WITHOUT waiting for
/// any TTL to elapse. This proves the self-healing dead-agent path without the
/// crate's forbidden time-based flakiness.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn claim_lazily_reclaims_seeded_past_lease_without_sleep() {
    let (_tmp, pool) = open_on_disk_pool().await;
    let story = seed_chain_to_story(&pool).await;
    let sprint = seed_sprint(&pool).await;
    let task = seed_queue_task(&pool, &story, &sprint, "Stale", "implement", Some("deep")).await;

    // Seed an ALREADY-EXPIRED lease: in_progress, owned by a dead agent, with a
    // lease_expires_at fixed in the past. No sleep — the timestamp is expired
    // the instant the test runs. (Raw runtime sqlx UPDATE — not a macro.)
    sqlx::query(
        "UPDATE work_items SET status = 'in_progress', assignee = 'dead-agent', \
         lease_expires_at = '2000-01-01 00:00:00' WHERE id = $1",
    )
    .bind(&task)
    .execute(&pool)
    .await
    .expect("seed expired lease in the past");

    // The next claim must (1) lazily reclaim the stale lease, then (2) re-lease
    // the now-reclaimable task to the live agent — all in the one call.
    let claimed: ClaimedTask =
        repo::claim_next_task(&pool, &sprint, Lane::Implement, None, "live-agent", LEASE_TTL_SECS)
            .await
            .expect("claim runs without error")
            .expect("the expired-lease task is reclaimed and then claimable");

    assert_eq!(claimed.task_id, task, "the reclaimed task is the one re-leased");
    assert_eq!(
        claimed.assignee, "live-agent",
        "the stale lease was reclaimed and re-leased to the new claimer"
    );

    // The DB reflects the hand-off: in_progress, owned by live-agent (not the
    // dead one), with a fresh (non-past) lease. (Raw runtime sqlx read.)
    use sqlx::Row as _;
    let row = sqlx::query("SELECT status, assignee, lease_expires_at FROM work_items WHERE id = $1")
        .bind(&task)
        .fetch_one(&pool)
        .await
        .expect("read reclaimed task row");
    let status: String = row.try_get("status").unwrap();
    let assignee: Option<String> = row.try_get("assignee").unwrap();
    let lease: Option<String> = row.try_get("lease_expires_at").unwrap();
    assert_eq!(status, "in_progress", "the reclaimed task is leased again");
    assert_eq!(
        assignee.as_deref(),
        Some("live-agent"),
        "ownership moved off the dead agent to the live claimer"
    );
    let lease = lease.expect("a fresh lease deadline is stamped");
    assert_ne!(
        lease, "2000-01-01 00:00:00",
        "the stale past deadline was replaced by a fresh one"
    );
}
