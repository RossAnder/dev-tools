//! Cross-plane e2e (Task 9, ADR-0006 Step 1b): control plane (lumina-server)
//! ↔ execution plane (lumina-companion) over the REAL WebSocket transport,
//! against a REAL temp git repository.
//!
//! This is the repo's first ephemeral-listener e2e. The companion WS handler
//! extracts `ConnectInfo<SocketAddr>` (its loopback-only guard), which the
//! in-process `oneshot` idiom cannot provide — so each scenario binds
//! `127.0.0.1:0` and serves the router exactly as `app::serve` does
//! (`into_make_service_with_connect_info::<SocketAddr>()`). Everything else
//! stays in-process and deterministic:
//!
//!   * the companion runs IN-PROCESS — `connection::run` (the production dial
//!     loop) spawned on a tokio task with `ShellGit` rooted at the temp repo,
//!     dialing `ws://127.0.0.1:{port}/api/companion/ws`;
//!   * registration is awaited via the registry's `connected()` watch channel
//!     (NO sleeps);
//!   * the HTTP layer (`POST /api/worktrees/{id}/execute-merge`) is driven via
//!     `tower::ServiceExt::oneshot` against a CLONE of the SAME router whose
//!     other clone rides the listener — both share one `AppState`, so the
//!     oneshot-driven flow sees the live companion. Chosen over a real HTTP
//!     client because reqwest/hyper are not in the dep tree and `oneshot` is
//!     the crate's established e2e idiom; only the WS leg needs the socket.
//!
//! Git fixture notes (pattern copied from `lumina/companion/tests/shell_git.rs`):
//!
//!   * the feature branch lives in a LINKED worktree sibling to the repo, with
//!     its commits made by test-side git plumbing;
//!   * the PRIMARY checkout is DETACHED before a merge scenario starts — the
//!     executor merges inside a dedicated integration worktree it attaches to
//!     the EXISTING target branch, and git refuses to attach a branch that is
//!     checked out elsewhere (the primary sitting on `main` would block it);
//!   * NO repo-link `local_path` is seeded, BY CHOICE: the execute-merge
//!     pre-flight's repo-root split-brain guard is SKIPPED when the project's
//!     primary repo-link `local_path` is unset, which keeps the temp-repo root
//!     out of the (lexically-normalised) clone-dir comparison.
//!
//! Each scenario stands up its own listener + AppState + temp repo (per-test
//! isolation), and aborts the companion + server tasks at the end so nextest's
//! process-per-test isolation reaps everything. Requires `git` on PATH.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tempfile::TempDir;
use tower::ServiceExt as _; // for `oneshot`

use lumina_companion::connection::{self, ConnectionConfig};
use lumina_companion::executor::Executor;
use lumina_companion::git::ShellGit;
use lumina_core::db::{AnyPool, connect_in_memory};
use lumina_core::domain::{
    NewSprint, NewWorktree, SprintStatus, TaskCommitQuery, WorktreeOutcome,
};
use lumina_core::repo;
use lumina_protocol::{Intent, Outcome};
use lumina_server::app::{AppState, build_router};

// ---------------------------------------------------------------------------
// Git fixture (the shell_git.rs tempdir pattern)
// ---------------------------------------------------------------------------

/// Run `git -C <dir> <args>` (test-side plumbing, NOT through the companion),
/// asserting success; returns trimmed stdout.
async fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("LC_ALL", "C")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .args(args)
        .output()
        .await
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {:?} in {} failed:\n{}",
        args,
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Stage everything and commit in `dir`; returns the new HEAD sha.
async fn commit_in(dir: &Path, msg: &str) -> String {
    git_in(dir, &["add", "-A"]).await;
    git_in(dir, &["commit", "-m", msg]).await;
    git_in(dir, &["rev-parse", "HEAD"]).await
}

/// `git merge-base --is-ancestor <ancestor> <descendant>` as a bool (exit 0 =
/// true, exit 1 = false) — the ground-truth reachability check.
async fn is_ancestor(dir: &Path, ancestor: &str, descendant: &str) -> bool {
    tokio::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .status()
        .await
        .expect("spawn git merge-base")
        .success()
}

/// One isolated repo (`<tempdir>/repo`) with a single `initial` commit on
/// `main` touching `file.txt`. The tempdir also hosts the linked worktrees.
struct TestRepo {
    tmp: TempDir,
    root: PathBuf,
}

impl TestRepo {
    async fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("repo");
        std::fs::create_dir(&root).expect("mkdir repo");
        git_in(&root, &["init", "-b", "main"]).await;
        git_in(&root, &["config", "user.name", "test"]).await;
        git_in(&root, &["config", "user.email", "test@example.com"]).await;
        git_in(&root, &["config", "core.autocrlf", "false"]).await;
        git_in(&root, &["config", "commit.gpgsign", "false"]).await;
        let repo = TestRepo { tmp, root };
        repo.write("file.txt", "base\n");
        repo.commit("initial").await;
        repo
    }

    fn write(&self, rel: &str, content: &str) {
        std::fs::write(self.root.join(rel), content).expect("write file");
    }

    async fn commit(&self, msg: &str) -> String {
        commit_in(&self.root, msg).await
    }

    /// A worktree path SIBLING to the repo inside the same tempdir.
    fn wt_path(&self, name: &str) -> PathBuf {
        self.tmp.path().join(name)
    }

    /// Create the `feature` branch from `main` in a linked worktree and
    /// return its path (test-side plumbing — the companion never sets this up).
    async fn add_feature_worktree(&self, name: &str) -> PathBuf {
        let wt = self.wt_path(name);
        let wt_str = wt.to_str().expect("utf-8 tempdir path").to_owned();
        git_in(&self.root, &["worktree", "add", "-b", "feature", &wt_str, "main"]).await;
        wt
    }

    /// Detach the PRIMARY checkout's HEAD so `main` is checked out NOWHERE —
    /// the executor's integration worktree can then attach the target branch
    /// (git refuses to check a branch out twice).
    async fn detach_primary(&self) {
        git_in(&self.root, &["checkout", "--detach"]).await;
    }
}

// ---------------------------------------------------------------------------
// Server + in-process companion stack
// ---------------------------------------------------------------------------

/// One scenario's full cross-plane stack: ephemeral listener serving the
/// router (real WS leg), a clone of the SAME router for `oneshot` HTTP, and
/// the production companion dial loop running in-process over `ShellGit`.
struct Stack {
    state: AppState,
    router: axum::Router,
    server: tokio::task::JoinHandle<()>,
    companion: tokio::task::JoinHandle<std::convert::Infallible>,
}

impl Stack {
    /// Bind `127.0.0.1:0`, serve the router with ConnectInfo (the companion WS
    /// route's loopback guard requires it), spawn `connection::run` rooted at
    /// `repo_root`, and await the registry's `connected()` watch — the
    /// deterministic registration signal (no sleeps).
    async fn spawn(pool: sqlx::SqlitePool, repo_root: &Path) -> Stack {
        let state = AppState::new(Arc::new(AnyPool::from(pool)));
        let router = build_router(state.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral listener");
        let port = listener.local_addr().expect("local addr").port();
        let server = tokio::spawn({
            let app = router.clone();
            async move {
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await
                .expect("server task");
            }
        });

        let executor = Executor::new(repo_root.to_path_buf(), Arc::new(ShellGit::new(repo_root)));
        let config = ConnectionConfig {
            server_url: format!("ws://127.0.0.1:{port}/api/companion/ws"),
            companion_id: "companion-e2e".to_owned(),
            repo_root: repo_root.display().to_string(),
        };
        let companion = tokio::spawn(connection::run(config, executor));

        let mut connected = state.companion.connected();
        connected
            .wait_for(|c| *c)
            .await
            .expect("connected watch closed before registration");

        Stack { state, router, server, companion }
    }

    /// Abort both tasks so nothing outlives the test body (nextest's
    /// process-per-test isolation reaps the rest).
    fn shutdown(self) {
        self.companion.abort();
        self.server.abort();
    }
}

/// `POST /api/worktrees/{id}/execute-merge` with an empty body (`no_ff`
/// defaults true, target defaults to the recorded `base_ref`), driven via
/// `oneshot` on a clone of the live router. Returns (status, JSON body).
async fn execute_merge(
    router: &axum::Router,
    worktree_id: &str,
) -> (StatusCode, serde_json::Value) {
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/worktrees/{worktree_id}/execute-merge"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .expect("oneshot");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let body = serde_json::from_slice(&bytes).expect("parse json body");
    (status, body)
}

// ---------------------------------------------------------------------------
// DB seeding (mirrors the sibling http/worktrees.rs test helpers)
// ---------------------------------------------------------------------------

/// Seed project→epic(+close criterion)→focus→story, then one task per title;
/// returns the task ids in input order.
async fn seed_story_with_tasks(pool: &sqlx::SqlitePool, task_titles: &[&str]) -> Vec<String> {
    let project = repo::create_work_item(pool, "project", None, "P", None)
        .await
        .expect("project");
    let epic = repo::create_work_item_full(
        pool,
        "epic",
        Some(&project.to_string()),
        "E",
        None,
        repo::CreateOpts { origin: None, outcome: Some("the epic outcome"), shape: None, lane: None },
    )
    .await
    .expect("epic");
    repo::add_acceptance_criterion(pool, &epic.to_string(), "epic close criterion")
        .await
        .expect("epic close criterion");
    let focus = repo::create_work_item_full(
        pool,
        "focus",
        Some(&epic.to_string()),
        "FO",
        None,
        repo::CreateOpts { origin: None, outcome: None, shape: Some("vertical-slice"), lane: None },
    )
    .await
    .expect("focus");
    let story = repo::create_work_item(pool, "story", Some(&focus.to_string()), "S", None)
        .await
        .expect("story");
    let mut tasks = Vec::with_capacity(task_titles.len());
    for title in task_titles {
        tasks.push(
            repo::create_work_item(pool, "task", Some(&story.to_string()), title, None)
                .await
                .expect("task")
                .to_string(),
        );
    }
    tasks
}

/// Create a sprint plus the worktree it owns (`branch="feature"`,
/// `base_ref="main"`, `path` = the linked feature worktree); returns
/// `(sprint_id, worktree_id)`. The sprint is left at its `'draft'` default.
async fn seed_sprint_with_worktree(
    pool: &sqlx::SqlitePool,
    feature_wt: &Path,
) -> (String, String) {
    let sprint = repo::create_sprint(
        pool,
        &NewSprint { title: None, worktree_id: None, predecessor_sprint_id: None },
    )
    .await
    .expect("sprint")
    .to_string();
    let wt = repo::create_worktree(
        pool,
        &NewWorktree {
            owning_sprint_id: sprint.clone(),
            path: feature_wt.display().to_string(),
            base_ref: Some("main".to_owned()),
            branch: Some("feature".to_owned()),
        },
    )
    .await
    .expect("worktree")
    .to_string();
    (sprint, wt)
}

/// Walk a freshly-created (`'draft'`) sprint to `'review'` through the TYPED
/// lifecycle (`draft → ready → active → review`) — the merge-eligible status.
async fn walk_sprint_to_review(pool: &sqlx::SqlitePool, sprint_id: &str) {
    for next in [SprintStatus::Ready, SprintStatus::Active, SprintStatus::Review] {
        repo::set_sprint_status(pool, sprint_id, next)
            .await
            .expect("walk sprint status");
    }
}

// ---------------------------------------------------------------------------
// Scenario 1: clean merge — outcome carries the ground-truth sha, git agrees,
// DB records merged + owner flips to done.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn execute_merge_e2e_records_ground_truth_and_git_agrees() {
    // Git: main@initial; feature worktree with TWO commits; primary detached.
    let repo = TestRepo::new().await;
    let feature_wt = repo.add_feature_worktree("wt-feature").await;
    std::fs::write(feature_wt.join("feature-a.txt"), "a\n").expect("write a");
    let sha_a = commit_in(&feature_wt, "feature a").await;
    std::fs::write(feature_wt.join("feature-b.txt"), "b\n").expect("write b");
    let sha_b = commit_in(&feature_wt, "feature b").await;
    repo.detach_primary().await;

    // DB: story + two tasks, sprint owning the worktree, walked to 'review'.
    let pool = connect_in_memory().await.expect("pool");
    let tasks = seed_story_with_tasks(&pool, &["TA", "TB"]).await;
    let (sprint, wt_id) = seed_sprint_with_worktree(&pool, &feature_wt).await;
    repo::add_tasks_to_sprint(&pool, &sprint, &[tasks[0].as_str(), tasks[1].as_str()])
        .await
        .expect("bind tasks to sprint");
    walk_sprint_to_review(&pool, &sprint).await;

    // Seed BOTH reachability join paths: sha_a rides arm (i) (`sprint_id` set);
    // sha_b rides arm (ii) (NULL `sprint_id`, its task bound via sprint_tasks).
    repo::record_task_commits(&pool, &sha_a, &[tasks[0].as_str()], Some(&sprint))
        .await
        .expect("record sha_a (sprint_id set)");
    repo::record_task_commits(&pool, &sha_b, &[tasks[1].as_str()], None)
        .await
        .expect("record sha_b (NULL sprint_id, via sprint_tasks)");
    let reachable = repo::list_worktree_reachable_shas(&pool, &wt_id)
        .await
        .expect("reachable shas");
    assert!(
        reachable.contains(&sha_a) && reachable.contains(&sha_b) && reachable.len() == 2,
        "both seeding routes feed must_remain_reachable: {reachable:?}"
    );

    // Stack up; drive the HTTP mirror of execute_worktree_merge.
    let stack = Stack::spawn(pool.clone(), &repo.root).await;
    let (status, body) = execute_merge(&stack.router, &wt_id).await;
    assert_eq!(status, StatusCode::OK, "execute-merge failed: {body}");
    assert_eq!(body["outcome"], "merged", "true merge (no_ff defaults true): {body}");
    assert_eq!(body["recorded"], true);
    assert_eq!(body["fast_forward"], false, "no_ff forces a merge commit");
    let merge_sha = body["merge_sha"].as_str().expect("merge_sha string").to_owned();

    // Git ground truth: the outcome's sha IS the new target tip, and every
    // recorded sha stayed reachable from it (the ADR §H stability gate held).
    assert_eq!(
        git_in(&repo.root, &["rev-parse", "main"]).await,
        merge_sha,
        "outcome merge_sha is main's tip"
    );
    assert!(is_ancestor(&repo.root, &sha_a, &merge_sha).await, "sha_a reachable");
    assert!(is_ancestor(&repo.root, &sha_b, &merge_sha).await, "sha_b reachable");

    // DB caught up with ground truth: audit stamped, owner 'review' → 'done'.
    let row = repo::get_worktree(&pool, &wt_id).await.expect("get_worktree");
    assert_eq!(row.merge_ref.as_deref(), Some(merge_sha.as_str()), "ground-truth sha recorded");
    assert!(row.merged_at.is_some(), "merged_at stamped");
    assert_eq!(row.outcome, Some(WorktreeOutcome::Merged));
    assert_eq!(row.effective_status, SprintStatus::Done, "owner flipped to done");

    stack.shutdown();
}

// ---------------------------------------------------------------------------
// Scenario 2: conflicting merge — Conflicted{paths}, NO DB write, lease
// released (a second attempt is not blocked by a stale lease).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn execute_merge_e2e_conflict_records_nothing_and_releases_lease() {
    // Git: diverge main and feature on the SAME file so the merge conflicts.
    let repo = TestRepo::new().await;
    let feature_wt = repo.add_feature_worktree("wt-feature").await;
    std::fs::write(feature_wt.join("file.txt"), "feature change\n").expect("write feature side");
    commit_in(&feature_wt, "feature change").await;
    repo.write("file.txt", "main change\n");
    let main_tip = repo.commit("main change").await;
    repo.detach_primary().await;

    // DB: sprint + worktree at 'review'. No tasks / recorded commits — an
    // empty must_remain_reachable set is fine, and with no sprint-bound task
    // the split-brain guard resolves no project binding and is skipped.
    let pool = connect_in_memory().await.expect("pool");
    let (sprint, wt_id) = seed_sprint_with_worktree(&pool, &feature_wt).await;
    walk_sprint_to_review(&pool, &sprint).await;

    let stack = Stack::spawn(pool.clone(), &repo.root).await;
    let (status, body) = execute_merge(&stack.router, &wt_id).await;
    assert_eq!(status, StatusCode::OK, "a conflict is a SUCCESS payload: {body}");
    assert_eq!(body["outcome"], "conflicted");
    assert_eq!(body["recorded"], false);
    assert_eq!(body["paths"][0], "file.txt", "conflicted path surfaced: {body}");

    // Git ground truth: the companion already aborted — main's tip unchanged.
    assert_eq!(
        git_in(&repo.root, &["rev-parse", "main"]).await,
        main_tip,
        "abort restored the pre-merge target tip"
    );

    // NO DB write: audit unstamped, owner still 'review'.
    let row = repo::get_worktree(&pool, &wt_id).await.expect("get_worktree");
    assert!(row.outcome.is_none(), "conflict records no outcome");
    assert!(row.merged_at.is_none(), "conflict stamps no merged_at");
    assert!(row.merge_ref.is_none(), "conflict records no merge_ref");
    assert_eq!(row.effective_status, SprintStatus::Review, "owner stays 'review'");

    // Lease released on the conflicted exit path: a SECOND execute attempt is
    // not refused as "already in flight" — it runs and conflicts again.
    let (status, body) = execute_merge(&stack.router, &wt_id).await;
    assert_eq!(status, StatusCode::OK, "second attempt not lease-blocked: {body}");
    assert_eq!(body["outcome"], "conflicted", "re-run reaches the merge again: {body}");

    stack.shutdown();
}

// ---------------------------------------------------------------------------
// Scenario 3: CommitCheckpoint inversion through the seam — companion mints
// the ground-truth commit, the server records it as task provenance
// (User Decision 4's internal demonstration).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn commit_checkpoint_e2e_inverts_into_recorded_provenance() {
    // Git: a feature worktree with UNCOMMITTED changes (the checkpoint input).
    let repo = TestRepo::new().await;
    let feature_wt = repo.add_feature_worktree("wt-feature").await;
    std::fs::write(feature_wt.join("checkpoint.txt"), "wip\n").expect("dirty the worktree");

    // DB: one task + a sprint to hang the provenance off (no merge here, so
    // the sprint can stay at its 'draft' default).
    let pool = connect_in_memory().await.expect("pool");
    let tasks = seed_story_with_tasks(&pool, &["T1"]).await;
    let sprint = repo::create_sprint(
        &pool,
        &NewSprint { title: None, worktree_id: None, predecessor_sprint_id: None },
    )
    .await
    .expect("sprint")
    .to_string();

    let stack = Stack::spawn(pool.clone(), &repo.root).await;

    // Drive the seam directly: one coarse CommitCheckpoint intent (commit-all
    // semantics) over the live WS connection.
    let outcome = stack
        .state
        .companion
        .execute(Intent::CommitCheckpoint {
            path: feature_wt.display().to_string(),
            message: "checkpoint: companion e2e".to_owned(),
        })
        .await
        .expect("companion execute");
    let Outcome::Checkpointed { commit_sha } = outcome else {
        panic!("expected Checkpointed, got {outcome:?}");
    };
    let sha = commit_sha.0;

    // Git ground truth: the returned sha IS the worktree's new HEAD.
    assert_eq!(
        git_in(&feature_wt, &["rev-parse", "HEAD"]).await,
        sha,
        "Checkpointed carries the worktree's new HEAD"
    );

    // Invert into the record store: ground-truth sha → task provenance row.
    let recorded = repo::record_task_commits(&pool, &sha, &[tasks[0].as_str()], Some(&sprint))
        .await
        .expect("record checkpoint provenance");
    assert_eq!(recorded, 1, "one new provenance edge");
    let commits = repo::list_task_commits(&pool, TaskCommitQuery::ByTask(tasks[0].clone()))
        .await
        .expect("list_task_commits");
    assert_eq!(commits.len(), 1, "the checkpoint edge reads back");
    assert_eq!(commits[0].commit_sha, sha);
    assert_eq!(commits[0].sprint_id.as_deref(), Some(sprint.as_str()));

    // Idempotent re-run on the now-clean worktree: AlreadyUpToDate with the
    // unchanged HEAD (the protocol's nothing-to-commit contract).
    let rerun = stack
        .state
        .companion
        .execute(Intent::CommitCheckpoint {
            path: feature_wt.display().to_string(),
            message: "checkpoint: rerun".to_owned(),
        })
        .await
        .expect("companion execute rerun");
    assert_eq!(
        rerun,
        Outcome::AlreadyUpToDate { tip: lumina_protocol::Sha(sha) },
        "clean-tree checkpoint reports AlreadyUpToDate with the unchanged tip"
    );

    stack.shutdown();
}
