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
//!   * the PRIMARY checkout STAYS on the target branch (`main`) throughout —
//!     the executor merges inside a DETACHED integration worktree and advances
//!     the branch ref via the post-merge compare-and-swap, so the operator's
//!     checkout never blocks a merge. The merge scenario asserts this
//!     REGRESSION directly (the old attach-to-target choreography failed here
//!     with `Failed{BranchInUse}`) plus the resulting stale-checkout operator
//!     hint (`target_checkout` + the `git reset --keep` remedy string);
//!   * NO repo-link `local_path` is seeded, BY CHOICE, in the merge/create
//!     scenarios: the execute pre-flights' repo-root split-brain guard is
//!     SKIPPED when the project's primary repo-link `local_path` is unset,
//!     which keeps the temp-repo root out of the (lexically-normalised)
//!     clone-dir comparison. Scenario 5 is the deliberate EXCEPTION — it
//!     seeds `local_path` precisely to exercise the guard's REJECTION arm
//!     (identity equality, review R13) and its identity-pass arm.
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
use lumina_protocol::{FailureKind, Intent, Outcome, ServerToCompanion};
use lumina_server::app::{AppState, build_router};
use lumina_server::companion::CompanionRegistry;

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

/// POST `uri` with a JSON `body`, driven via `oneshot` on a clone of the live
/// router. Returns (status, JSON body).
async fn post_json(
    router: &axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
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

/// `POST /api/worktrees/{id}/execute-merge` with an empty body (`no_ff`
/// defaults true, target defaults to the recorded `base_ref`).
async fn execute_merge(
    router: &axum::Router,
    worktree_id: &str,
) -> (StatusCode, serde_json::Value) {
    post_json(
        router,
        &format!("/api/worktrees/{worktree_id}/execute-merge"),
        serde_json::json!({}),
    )
    .await
}

/// `POST /api/sprints/{sprint_id}/worktree/execute` — the execute-create
/// mirror (`branch` + `base_ref` both REQUIRED by the body contract).
async fn execute_create(
    router: &axum::Router,
    sprint_id: &str,
    branch: &str,
    base_ref: &str,
) -> (StatusCode, serde_json::Value) {
    post_json(
        router,
        &format!("/api/sprints/{sprint_id}/worktree/execute"),
        serde_json::json!({ "branch": branch, "base_ref": base_ref }),
    )
    .await
}

// ---------------------------------------------------------------------------
// DB seeding (mirrors the sibling http/worktrees.rs test helpers)
// ---------------------------------------------------------------------------

/// Seed project→epic(+close criterion)→focus→story, then one task per title;
/// returns `(project_id, task_ids)` (tasks in input order). The project id is
/// what the split-brain scenario hangs its primary repo-link off.
async fn seed_story_with_tasks(
    pool: &sqlx::SqlitePool,
    task_titles: &[&str],
) -> (String, Vec<String>) {
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
    (project.to_string(), tasks)
}

/// Create one bare sprint (left at its `'draft'` default); returns its id.
async fn seed_sprint(pool: &sqlx::SqlitePool) -> String {
    repo::create_sprint(
        pool,
        &NewSprint { title: None, worktree_id: None, predecessor_sprint_id: None },
    )
    .await
    .expect("sprint")
    .to_string()
}

/// Create a sprint plus the worktree it owns (`branch="feature"`,
/// `base_ref="main"`, `path` = the linked feature worktree); returns
/// `(sprint_id, worktree_id)`. The sprint is left at its `'draft'` default.
async fn seed_sprint_with_worktree(
    pool: &sqlx::SqlitePool,
    feature_wt: &Path,
) -> (String, String) {
    let sprint = seed_sprint(pool).await;
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
// Scenario 1: clean merge WITH the primary checkout sitting ON the target
// branch — the detached-integration ref-CAS regression (the old attach-to-
// target choreography failed this exact setup with `Failed{BranchInUse}`).
// The outcome carries the ground-truth sha + the stale-checkout operator
// hint, git agrees, DB records merged + owner flips to done, and the primary
// WORKING TREE is untouched (ref moved, files didn't).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn execute_merge_e2e_records_ground_truth_and_git_agrees() {
    // Git: main@initial; feature worktree with TWO commits. The PRIMARY
    // checkout deliberately STAYS on `main` (the merge target) — the
    // regression under test: this used to be the BranchInUse wall.
    let repo = TestRepo::new().await;
    let feature_wt = repo.add_feature_worktree("wt-feature").await;
    std::fs::write(feature_wt.join("feature-a.txt"), "a\n").expect("write a");
    let sha_a = commit_in(&feature_wt, "feature a").await;
    std::fs::write(feature_wt.join("feature-b.txt"), "b\n").expect("write b");
    let sha_b = commit_in(&feature_wt, "feature b").await;

    // DB: story + two tasks, sprint owning the worktree, walked to 'review'.
    let pool = connect_in_memory().await.expect("pool");
    let (_project, tasks) = seed_story_with_tasks(&pool, &["TA", "TB"]).await;
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

    // The stale-checkout operator hint: the target branch was checked out in
    // the primary checkout when the ref advanced, so the payload carries the
    // structured `target_checkout` field naming that checkout (clean here)
    // plus the human remedy string.
    let hint_path = body["target_checkout"]["path"]
        .as_str()
        .unwrap_or_else(|| panic!("structured target_checkout.path present: {body}"));
    assert_eq!(
        std::fs::canonicalize(hint_path).expect("canonicalize hint path"),
        std::fs::canonicalize(&repo.root).expect("canonicalize repo root"),
        "the hint names the primary checkout"
    );
    assert_eq!(
        body["target_checkout"]["dirty"], false,
        "the primary checkout was clean at the pre-merge snapshot: {body}"
    );
    let hint = body["hint"].as_str().expect("human hint string present");
    assert!(
        hint.contains("git reset --keep") && hint.contains(&merge_sha),
        "the hint carries the `git reset --keep <merge_sha>` remedy: {hint}"
    );

    // Stale-checkout effect on disk: the REF moved, the primary WORKING TREE
    // did not — still attached to `main`, files exactly as before the merge.
    assert_eq!(
        git_in(&repo.root, &["symbolic-ref", "--short", "HEAD"]).await,
        "main",
        "the primary checkout stayed attached to main"
    );
    assert_eq!(
        std::fs::read_to_string(repo.root.join("file.txt")).expect("read file.txt"),
        "base\n",
        "primary working-tree content untouched"
    );
    assert!(
        !repo.root.join("feature-a.txt").exists() && !repo.root.join("feature-b.txt").exists(),
        "merged files did NOT materialise in the primary working tree"
    );

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
    // The primary checkout stays ON `main` — under the detached-integration
    // choreography that no longer affects conflict semantics either.
    let repo = TestRepo::new().await;
    let feature_wt = repo.add_feature_worktree("wt-feature").await;
    std::fs::write(feature_wt.join("file.txt"), "feature change\n").expect("write feature side");
    commit_in(&feature_wt, "feature change").await;
    repo.write("file.txt", "main change\n");
    let main_tip = repo.commit("main change").await;

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
    let (_project, tasks) = seed_story_with_tasks(&pool, &["T1"]).await;
    let sprint = seed_sprint(&pool).await;

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

// ---------------------------------------------------------------------------
// Scenario 4: execute-create — the companion creates the worktree on disk
// (resolving the committish base itself), the server records the GROUND-TRUTH
// path; a duplicate live branch is refused on BOTH planes (git's BranchInUse
// → 502 companion envelope; the migration-0018 partial UNIQUE index → 422).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn execute_create_e2e_records_ground_truth_and_rejects_duplicate_branch() {
    let repo = TestRepo::new().await;
    let main_tip = git_in(&repo.root, &["rev-parse", "main"]).await;

    // DB: a bare sprint at its 'draft' default — non-terminal, so the create
    // pre-flight admits it. No tasks / repo-links, so the split-brain
    // repo-root guard resolves no project binding and is skipped.
    let pool = connect_in_memory().await.expect("pool");
    let sprint_a = seed_sprint(&pool).await;

    let stack = Stack::spawn(pool.clone(), &repo.root).await;

    // (a) Happy path: 200 + { worktree_id, path, head }.
    let (status, body) = execute_create(&stack.router, &sprint_a, "sprint/alpha", "main").await;
    assert_eq!(status, StatusCode::OK, "execute-create failed: {body}");
    let worktree_id = body["worktree_id"].as_str().expect("worktree_id string").to_owned();
    let path = body["path"].as_str().expect("path string").to_owned();
    let head = body["head"].as_str().expect("head string").to_owned();

    // Git ground truth: `head` is the companion-RESOLVED `main` tip, and the
    // worktree sits on disk at the SANITISED companion-managed location
    // (`sprint/alpha` → `.lumina/worktrees/sprint-alpha`), checked out on the
    // NEW branch at that tip.
    assert_eq!(head, main_tip, "head == git rev-parse main");
    let wt_path = PathBuf::from(&path);
    assert_eq!(
        std::fs::canonicalize(&wt_path).expect("worktree exists on disk"),
        std::fs::canonicalize(repo.root.join(".lumina").join("worktrees").join("sprint-alpha"))
            .expect("canonicalize expected managed path"),
        "the companion-chosen path is the sanitised managed location"
    );
    assert_eq!(
        git_in(&wt_path, &["rev-parse", "HEAD"]).await,
        main_tip,
        "the new worktree's HEAD commit is main's tip"
    );
    assert_eq!(
        git_in(&wt_path, &["symbolic-ref", "--short", "HEAD"]).await,
        "sprint/alpha",
        "the worktree is checked out on the NEW branch (started at the resolved base)"
    );

    // DB ground truth: the row records the companion's path + the owner link.
    let row = repo::get_worktree(&pool, &worktree_id).await.expect("get_worktree");
    assert_eq!(row.path, path, "the GROUND-TRUTH path is what got recorded");
    assert_eq!(row.owning_sprint_id, sprint_a, "owned by the requesting sprint");
    assert_eq!(row.branch.as_deref(), Some("sprint/alpha"));
    assert_eq!(row.base_ref.as_deref(), Some("main"));
    assert_eq!(row.effective_status, SprintStatus::Draft, "derived from the owner");

    // (b) A SECOND create for a DIFFERENT sprint with the SAME branch: the
    // EXECUTION plane refuses first — `sprint/alpha` already exists in git,
    // so the companion reports Failed{BranchInUse} and the handler maps it to
    // the 502 companion envelope. (The migration-0018 DB index cannot be
    // reached on this path: git fails before any record write.)
    let sprint_b = seed_sprint(&pool).await;
    let (status, body) = execute_create(&stack.router, &sprint_b, "sprint/alpha", "main").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "duplicate git branch is 502: {body}");
    assert_eq!(body["error"]["kind"], "companion");
    // R7: callers branch on the STRUCTURED snake_case failure_kind field, never
    // on rendered Debug prose (which no longer appears in the message).
    assert_eq!(
        body["error"]["failure_kind"], "branch_in_use",
        "the envelope carries the structured failure_kind: {body}"
    );
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("message string")
            .contains("branch_in_use"),
        "the message names the wire failure kind: {body}"
    );

    // (c) The migration-0018 RECORD-side guard through the SAME flow: a
    // record-only row (no git branch behind it) already holds `sprint/gamma`,
    // so the companion's git create SUCCEEDS but the record write hits the
    // partial live-branch UNIQUE index → a clean 422 validation envelope.
    let sprint_c = seed_sprint(&pool).await;
    repo::create_worktree(
        &pool,
        &NewWorktree {
            owning_sprint_id: sprint_c,
            path: repo.wt_path("recorded-only").display().to_string(),
            base_ref: Some("main".to_owned()),
            branch: Some("sprint/gamma".to_owned()),
        },
    )
    .await
    .expect("record-only worktree row");
    let sprint_d = seed_sprint(&pool).await;
    let (status, body) = execute_create(&stack.router, &sprint_d, "sprint/gamma", "main").await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "duplicate LIVE branch record is 422: {body}"
    );
    assert_eq!(body["error"]["kind"], "validation");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("message string")
            .contains("already records branch"),
        "the validation names the live-branch invariant: {body}"
    );

    // Neither failed create recorded anything: exactly TWO live rows exist —
    // sprint A's executed worktree and sprint C's record-only seed.
    let rows = repo::list_worktrees(&pool, None).await.expect("list_worktrees");
    assert_eq!(rows.len(), 2, "failed creates recorded nothing: {rows:?}");

    stack.shutdown();
}

// ---------------------------------------------------------------------------
// Scenario 5 (review R23): the split-brain guard's REJECTION arm — every
// other scenario leaves the primary repo-link `local_path` UNSET so the guard
// is skipped; this one seeds it deliberately. The guard is strict IDENTITY
// over `repo::normalise_path_for_compare` (review R13): (a) a SIBLING dir is
// rejected, (b) a NESTED repo_root under the clone dir — the case the old
// containment matcher wrongly ACCEPTED — is rejected too, and in both cases
// NOTHING is recorded and no git ran. (c) the exact clone dir passes the
// guard through the normaliser (a real Windows path: case + separators) and
// the merge proceeds — proving the rejections above were the guard, not some
// other pre-flight.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn execute_merge_split_brain_guard_rejects_mismatched_repo_root() {
    // Git: one feature commit, so phase (c)'s merge genuinely has work to do.
    let repo = TestRepo::new().await;
    let feature_wt = repo.add_feature_worktree("wt-feature").await;
    std::fs::write(feature_wt.join("feature.txt"), "x\n").expect("write feature file");
    commit_in(&feature_wt, "feature change").await;
    let main_tip = git_in(&repo.root, &["rev-parse", "main"]).await;

    // DB: a task BOUND to the owning sprint, so the worktree-keyed binding
    // resolves a project (no bound task ⇒ guard skipped — the default the
    // other scenarios rely on); a PRIMARY repo link to hang `local_path` off.
    let pool = connect_in_memory().await.expect("pool");
    let (project, tasks) = seed_story_with_tasks(&pool, &["T1"]).await;
    let (sprint, wt_id) = seed_sprint_with_worktree(&pool, &feature_wt).await;
    repo::add_tasks_to_sprint(&pool, &sprint, &[tasks[0].as_str()])
        .await
        .expect("bind task to sprint");
    walk_sprint_to_review(&pool, &sprint).await;
    let link = repo::add_repo_link(&pool, &project, "acme/widget", true)
        .await
        .expect("primary repo link")
        .to_string();

    // Companion rooted at the REAL repo — its Hello.repo_root is what the
    // guard compares against the seeded local_path.
    let stack = Stack::spawn(pool.clone(), &repo.root).await;

    // (a) SIBLING dir: any directory other than the repo root fails identity.
    let sibling = repo.tmp.path().join("elsewhere");
    repo::set_repo_local_path(&pool, &link, Some(sibling.to_str().expect("utf-8 path")))
        .await
        .expect("set sibling local_path");
    let (status, body) = execute_merge(&stack.router, &wt_id).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a mismatched repo_root is a 422 pre-flight Validation: {body}"
    );
    assert_eq!(body["error"]["kind"], "validation");
    let msg = body["error"]["message"].as_str().expect("message string");
    assert!(
        msg.contains("split-brain guard") && msg.contains("elsewhere"),
        "the rejection names the guard and the mismatched clone dir: {msg}"
    );

    // (b) NESTED (the R13 regression): local_path = the PARENT of the
    // companion's repo_root, so repo_root is a strict DESCENDANT of the clone
    // dir. The old containment matcher accepted exactly this; identity must
    // reject it.
    repo::set_repo_local_path(&pool, &link, Some(repo.tmp.path().to_str().expect("utf-8 path")))
        .await
        .expect("set parent local_path");
    let (status, body) = execute_merge(&stack.router, &wt_id).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a NESTED repo_root under the clone dir is rejected (identity, not containment): {body}"
    );
    assert_eq!(body["error"]["kind"], "validation");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("message string")
            .contains("split-brain guard"),
        "the nested rejection is the guard's: {body}"
    );

    // After BOTH rejections: NOTHING recorded, no git ran.
    let row = repo::get_worktree(&pool, &wt_id).await.expect("get_worktree");
    assert!(row.outcome.is_none(), "rejection records no outcome");
    assert!(row.merged_at.is_none(), "rejection stamps no merged_at");
    assert!(row.merge_ref.is_none(), "rejection records no merge_ref");
    assert_eq!(row.effective_status, SprintStatus::Review, "owner stays 'review'");
    assert_eq!(
        git_in(&repo.root, &["rev-parse", "main"]).await,
        main_tip,
        "no merge was dispatched — main's tip is unmoved"
    );

    // (c) IDENTITY: local_path = exactly the companion's repo_root (the same
    // string both sides derive from), run through the migration-0014
    // normaliser — the guard passes and the merge proceeds end-to-end.
    repo::set_repo_local_path(&pool, &link, Some(repo.root.to_str().expect("utf-8 path")))
        .await
        .expect("set identity local_path");
    let (status, body) = execute_merge(&stack.router, &wt_id).await;
    assert_eq!(status, StatusCode::OK, "identity passes the guard: {body}");
    assert_eq!(body["outcome"], "merged", "the merge ran once the guard passed: {body}");

    stack.shutdown();
}

// ---------------------------------------------------------------------------
// Scenario 6 (review R24): a lost ref-CAS — the companion's terminal
// `Failed{TargetMoved}` — maps through the server flow to the 502 companion
// envelope with the STRUCTURED snake_case `failure_kind`, and records
// NOTHING. The companion is STUBBED at the registry seam (the
// mcp/worktrees.rs stub pattern): a real companion cannot lose the CAS
// deterministically, so the test injects a `CompanionRegistry`, receives the
// MergeWorktree intent off the wire, and answers it with TargetMoved.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn execute_merge_target_moved_is_502_with_structured_failure_kind() {
    // DB only — no temp git repo and no real companion process.
    let pool = connect_in_memory().await.expect("pool");
    let (sprint, wt_id) = seed_sprint_with_worktree(&pool, Path::new("/tmp/wt-stub")).await;
    walk_sprint_to_review(&pool, &sprint).await;

    // Stub registry: register a test-held channel as the companion slot. No
    // task is bound to the sprint, so the split-brain guard is skipped.
    let reg = Arc::new(CompanionRegistry::new());
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let _token = reg.register(tx, "/work/repo".to_owned()).expect("slot free");
    let mut state = AppState::new(Arc::new(AnyPool::from(pool.clone())));
    state.companion = reg.clone();
    let router = build_router(state);

    // Drive the HTTP mirror concurrently; answer the intent with the lost-CAS
    // terminal failure.
    let handle = tokio::spawn({
        let router = router.clone();
        let wt_id = wt_id.clone();
        async move { execute_merge(&router, &wt_id).await }
    });
    let ServerToCompanion::IntentRequest { id, intent } =
        rx.recv().await.expect("intent on the wire");
    assert!(
        matches!(intent, Intent::MergeWorktree { .. }),
        "expected MergeWorktree, got {intent:?}"
    );
    reg.complete(
        id,
        Outcome::Failed {
            kind: FailureKind::TargetMoved,
            message: "target branch 'main' moved between tip-resolve and the ref-CAS advance"
                .to_owned(),
        },
    );

    // (a) The STRUCTURAL envelope: 502 (an execution-plane fault, NOT
    // caller-input — only NotFound/DirtyWorktree map to 422), kind=companion,
    // and the snake_case wire name riding the structured `failure_kind` field
    // (review R7: callers branch on this, never on message prose).
    let (status, body) = handle.await.expect("join");
    assert_eq!(
        status,
        StatusCode::BAD_GATEWAY,
        "TargetMoved is an execution-plane fault → 502: {body}"
    );
    assert_eq!(body["error"]["kind"], "companion");
    assert_eq!(
        body["error"]["failure_kind"], "target_moved",
        "the envelope carries the structured snake_case failure_kind: {body}"
    );

    // (b) NOTHING recorded: a terminal Failed leaves the audit unstamped and
    // the owner in 'review' — a re-run against the new tip stays possible.
    let row = repo::get_worktree(&pool, &wt_id).await.expect("get_worktree");
    assert!(row.outcome.is_none(), "TargetMoved records no outcome");
    assert!(row.merged_at.is_none(), "TargetMoved stamps no merged_at");
    assert!(row.merge_ref.is_none(), "TargetMoved records no merge_ref");
    assert_eq!(row.effective_status, SprintStatus::Review, "owner stays 'review'");
}
