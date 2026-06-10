//! Integration tests: [`ShellGit`] against REAL temp git repositories.
//!
//! Requires a `git` binary on PATH. Each test stands up an isolated repo
//! inside a `tempfile` tempdir — `git init -b main`, local identity, and
//! `core.autocrlf=false` so LF content is pinned across platforms — with
//! linked worktrees created as SIBLINGS of the repo in the same tempdir.
//! Task 9 (the companion e2e) copies this fixture pattern.

use std::path::{Path, PathBuf};

use lumina_companion::git::{
    GitBackend, GitError, MergeResult, ResolveOp, ResolveOutcome, Sha, ShellGit, WorktreeStatus,
};
use tempfile::TempDir;

/// A well-formed sha that names nothing in any fresh test repo.
const MISSING_SHA: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef";

/// Run `git -C <dir> <args>` (test-side plumbing, NOT through ShellGit),
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

    fn backend(&self) -> ShellGit {
        ShellGit::new(&self.root)
    }

    fn write(&self, rel: &str, content: &str) {
        std::fs::write(self.root.join(rel), content).expect("write file");
    }

    async fn commit(&self, msg: &str) -> String {
        commit_in(&self.root, msg).await
    }

    async fn head(&self) -> String {
        git_in(&self.root, &["rev-parse", "HEAD"]).await
    }

    /// A worktree path SIBLING to the repo inside the same tempdir.
    fn wt_path(&self, name: &str) -> PathBuf {
        self.tmp.path().join(name)
    }
}

/// Diverge `main` and a `feature` worktree on `file.txt` so a merge of
/// `feature` into `main` conflicts. Returns `(worktree_path, main_tip)`.
async fn conflicted_fixture(repo: &TestRepo, g: &ShellGit) -> (PathBuf, String) {
    let base = repo.head().await;
    let wt = repo.wt_path("wt-feature");
    g.create_worktree(&wt, "feature", &Sha::new(base.as_str()))
        .await
        .expect("create feature worktree");
    repo.write("file.txt", "main change\n");
    let main_tip = repo.commit("main change").await;
    std::fs::write(wt.join("file.txt"), "feature change\n").expect("write feature side");
    commit_in(&wt, "feature change").await;
    (wt, main_tip)
}

#[tokio::test]
async fn create_and_remove_worktree() {
    let repo = TestRepo::new().await;
    let g = repo.backend();
    let base = repo.head().await;
    let wt = repo.wt_path("wt-a");

    let state = g
        .create_worktree(&wt, "sprint-a", &Sha::new(base.as_str()))
        .await
        .expect("create_worktree");
    assert_eq!(state.path, wt);
    assert_eq!(state.branch.as_deref(), Some("sprint-a"));
    assert_eq!(state.head.as_str(), base);
    assert_eq!(state.status, WorktreeStatus::Clean);

    g.remove_worktree(&wt, false).await.expect("remove_worktree");
    assert!(!wt.exists(), "worktree dir should be gone after remove");
}

#[tokio::test]
async fn create_worktree_rejects_existing_branch_and_missing_start() {
    let repo = TestRepo::new().await;
    let g = repo.backend();
    let base = repo.head().await;

    git_in(&repo.root, &["branch", "taken"]).await;
    let err = g
        .create_worktree(&repo.wt_path("wt-x"), "taken", &Sha::new(base.as_str()))
        .await
        .unwrap_err();
    assert!(matches!(err, GitError::State(_)), "expected State, got {err:?}");

    let err = g
        .create_worktree(&repo.wt_path("wt-y"), "fresh", &Sha::new(MISSING_SHA))
        .await
        .unwrap_err();
    assert!(
        matches!(err, GitError::NotFound(_)),
        "expected NotFound, got {err:?}"
    );
}

#[tokio::test]
async fn attach_worktree_checks_out_existing_branch_at_its_tip() {
    let repo = TestRepo::new().await;
    let g = repo.backend();
    let base = repo.head().await;
    // `side` stays at the initial commit while `main` moves on — attaching
    // must land on the BRANCH tip, not main's.
    git_in(&repo.root, &["branch", "side"]).await;
    repo.write("file.txt", "main moved\n");
    let main_tip = repo.commit("main moved").await;
    assert_ne!(base, main_tip);

    let wt = repo.wt_path("wt-side");
    let head = g.attach_worktree("side", &wt).await.expect("attach_worktree");
    assert_eq!(head.as_str(), base);
    assert_eq!(git_in(&wt, &["symbolic-ref", "--short", "HEAD"]).await, "side");
}

#[tokio::test]
async fn attach_worktree_rejects_checked_out_branch_and_missing_branch() {
    let repo = TestRepo::new().await;
    let g = repo.backend();

    // `main` is checked out in the primary worktree — git refuses to check a
    // branch out twice ("already used by worktree"), classified State.
    let err = g
        .attach_worktree("main", &repo.wt_path("wt-main-again"))
        .await
        .unwrap_err();
    assert!(matches!(err, GitError::State(_)), "expected State, got {err:?}");

    // A missing branch resolves as an unknown committish ("invalid
    // reference"), classified NotFound.
    let err = g
        .attach_worktree("no-such-branch", &repo.wt_path("wt-missing"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, GitError::NotFound(_)),
        "expected NotFound, got {err:?}"
    );
}

#[tokio::test]
async fn remove_dirty_worktree_refuses_then_force_succeeds() {
    let repo = TestRepo::new().await;
    let g = repo.backend();
    let base = repo.head().await;
    let wt = repo.wt_path("wt-dirty");
    g.create_worktree(&wt, "dirty", &Sha::new(base.as_str()))
        .await
        .expect("create_worktree");
    std::fs::write(wt.join("untracked.txt"), "u\n").expect("write untracked");

    let err = g.remove_worktree(&wt, false).await.unwrap_err();
    assert!(matches!(err, GitError::State(_)), "expected State, got {err:?}");
    assert!(wt.exists(), "refused remove must leave the worktree in place");

    g.remove_worktree(&wt, true).await.expect("forced remove");
    assert!(!wt.exists());
}

#[tokio::test]
async fn clean_fast_forward_merge() {
    let repo = TestRepo::new().await;
    let g = repo.backend();
    let base = repo.head().await;
    let wt = repo.wt_path("wt-ff");
    g.create_worktree(&wt, "feature", &Sha::new(base.as_str()))
        .await
        .expect("create_worktree");
    std::fs::write(wt.join("feature.txt"), "x\n").expect("write");
    let tip = commit_in(&wt, "feature work").await;

    let res = g
        .merge(&repo.root, "feature", "main", false)
        .await
        .expect("merge");
    assert_eq!(
        res,
        MergeResult::FastForward {
            new_tip: Sha::new(tip.as_str())
        }
    );
    assert_eq!(repo.head().await, tip);
}

#[tokio::test]
async fn no_ff_merge_creates_a_true_merge_commit() {
    let repo = TestRepo::new().await;
    let g = repo.backend();
    let base = repo.head().await;
    let wt = repo.wt_path("wt-noff");
    g.create_worktree(&wt, "feature", &Sha::new(base.as_str()))
        .await
        .expect("create_worktree");
    std::fs::write(wt.join("feature.txt"), "x\n").expect("write");
    let tip = commit_in(&wt, "feature work").await;

    let res = g
        .merge(&repo.root, "feature", "main", true)
        .await
        .expect("merge --no-ff");
    let MergeResult::Merged { merge_sha } = res else {
        panic!("expected Merged, got {res:?}");
    };
    assert_ne!(merge_sha.as_str(), tip, "--no-ff must mint a NEW commit");
    assert_eq!(merge_sha.as_str(), repo.head().await);
    // Independent verification: the merge commit's second parent is the
    // feature tip, and the first parent is the pre-merge main tip.
    assert_eq!(git_in(&repo.root, &["rev-parse", "HEAD^2"]).await, tip);
    assert_eq!(git_in(&repo.root, &["rev-parse", "HEAD^1"]).await, base);
}

#[tokio::test]
async fn merge_of_an_already_merged_source_is_already_up_to_date() {
    let repo = TestRepo::new().await;
    let g = repo.backend();
    let base = repo.head().await;
    // `feature` points at main's tip: nothing to merge.
    g.create_worktree(&repo.wt_path("wt-same"), "feature", &Sha::new(base.as_str()))
        .await
        .expect("create_worktree");

    let res = g
        .merge(&repo.root, "feature", "main", false)
        .await
        .expect("merge");
    assert_eq!(res, MergeResult::AlreadyUpToDate);
    assert_eq!(repo.head().await, base, "HEAD must not move");
}

#[tokio::test]
async fn merge_requires_target_checked_out_in_worktree() {
    let repo = TestRepo::new().await;
    let g = repo.backend();
    let base = repo.head().await;
    let wt = repo.wt_path("wt-wrong");
    g.create_worktree(&wt, "feature", &Sha::new(base.as_str()))
        .await
        .expect("create_worktree");

    // `wt` has `feature` checked out — merging into target `main` there must
    // be refused before any engine call.
    let err = g.merge(&wt, "main", "main", false).await.unwrap_err();
    assert!(matches!(err, GitError::State(_)), "expected State, got {err:?}");

    // A missing source classifies as NotFound (checked after the branch gate).
    let err = g
        .merge(&repo.root, "no-such-branch", "main", false)
        .await
        .unwrap_err();
    assert!(
        matches!(err, GitError::NotFound(_)),
        "expected NotFound, got {err:?}"
    );
}

#[tokio::test]
async fn conflicting_merge_reports_paths_and_abort_restores_pre_state() {
    let repo = TestRepo::new().await;
    let g = repo.backend();
    let (_wt, main_tip) = conflicted_fixture(&repo, &g).await;

    let res = g
        .merge(&repo.root, "feature", "main", false)
        .await
        .expect("conflicting merge is an Ok(MergeResult), not a GitError");
    assert_eq!(
        res,
        MergeResult::Conflict {
            paths: vec!["file.txt".to_owned()]
        }
    );

    // State derivation sees the in-progress conflicted merge.
    let states = g.worktree_states().await.expect("worktree_states");
    let main_state = states
        .iter()
        .find(|s| s.branch.as_deref() == Some("main"))
        .expect("main worktree entry");
    assert_eq!(main_state.status, WorktreeStatus::Conflicted);

    // Abort restores the pre-merge HEAD and a clean tree.
    g.abort_merge(&repo.root).await.expect("abort_merge");
    assert_eq!(repo.head().await, main_tip);
    let states = g.worktree_states().await.expect("worktree_states");
    let main_state = states
        .iter()
        .find(|s| s.branch.as_deref() == Some("main"))
        .expect("main worktree entry");
    assert_eq!(main_state.status, WorktreeStatus::Clean);
    assert_eq!(
        std::fs::read_to_string(repo.root.join("file.txt")).expect("read"),
        "main change\n"
    );

    // A second abort has no merge to act on.
    let err = g.abort_merge(&repo.root).await.unwrap_err();
    assert!(matches!(err, GitError::State(_)), "expected State, got {err:?}");
}

#[tokio::test]
async fn resolve_take_theirs_then_continue_completes_the_merge() {
    let repo = TestRepo::new().await;
    let g = repo.backend();
    let (_wt, _main_tip) = conflicted_fixture(&repo, &g).await;

    let res = g
        .merge(&repo.root, "feature", "main", false)
        .await
        .expect("merge");
    assert!(matches!(res, MergeResult::Conflict { .. }), "got {res:?}");

    // Empty path list = "every currently-conflicted path".
    let res = g
        .resolve(&repo.root, ResolveOp::TakeTheirs { paths: vec![] })
        .await
        .expect("resolve TakeTheirs");
    assert_eq!(res, ResolveOutcome::Pending { remaining: vec![] });

    let res = g
        .resolve(&repo.root, ResolveOp::Continue)
        .await
        .expect("resolve Continue");
    let ResolveOutcome::Completed { merge_sha } = res else {
        panic!("expected Completed, got {res:?}");
    };
    assert_eq!(merge_sha.as_str(), repo.head().await);
    // "Theirs" (the feature side) won the conflicted file.
    assert_eq!(
        std::fs::read_to_string(repo.root.join("file.txt")).expect("read"),
        "feature change\n"
    );

    // With the merge concluded, resolve is a State error again.
    let err = g.resolve(&repo.root, ResolveOp::Continue).await.unwrap_err();
    assert!(matches!(err, GitError::State(_)), "expected State, got {err:?}");
}

#[tokio::test]
async fn is_ancestor_contract() {
    let repo = TestRepo::new().await;
    let g = repo.backend();
    let c1 = repo.head().await;
    repo.write("file.txt", "second\n");
    let c2 = repo.commit("second").await;

    assert!(
        g.is_ancestor(&Sha::new(c1.as_str()), &Sha::new(c2.as_str()))
            .await
            .expect("ancestor query")
    );
    assert!(
        !g.is_ancestor(&Sha::new(c2.as_str()), &Sha::new(c1.as_str()))
            .await
            .expect("descendant query")
    );
    let err = g
        .is_ancestor(&Sha::new(MISSING_SHA), &Sha::new(c2.as_str()))
        .await
        .unwrap_err();
    assert!(
        matches!(err, GitError::NotFound(_)),
        "expected NotFound, got {err:?}"
    );
}

#[tokio::test]
async fn commit_exists_and_head_of() {
    let repo = TestRepo::new().await;
    let g = repo.backend();
    let c1 = repo.head().await;

    assert!(g.commit_exists(&Sha::new(c1.as_str())).await.expect("exists"));
    assert!(!g.commit_exists(&Sha::new(MISSING_SHA)).await.expect("missing"));
    assert_eq!(g.head_of(&repo.root).await.expect("head_of").as_str(), c1);
}

#[tokio::test]
async fn commit_all_stages_everything_and_is_idempotent_on_clean() {
    let repo = TestRepo::new().await;
    let g = repo.backend();

    // Clean tree: nothing to commit, a normal outcome.
    assert_eq!(g.commit_all(&repo.root, "noop").await.expect("clean"), None);

    repo.write("new.txt", "n\n"); // untracked — `-A` must pick it up
    repo.write("file.txt", "edited\n"); // modified
    let sha = g
        .commit_all(&repo.root, "checkpoint")
        .await
        .expect("commit_all")
        .expect("Some(sha) when there were changes");
    assert_eq!(sha.as_str(), repo.head().await);
    // The companion's stable identity is stamped via env, not repo config.
    assert_eq!(
        git_in(&repo.root, &["log", "-1", "--format=%an <%ae>"]).await,
        "lumina-companion <companion@lumina.local>"
    );

    // Re-run on the now-clean tree: idempotent None.
    assert_eq!(g.commit_all(&repo.root, "again").await.expect("rerun"), None);
}

#[tokio::test]
async fn reset_hard_discards_commits_and_local_changes() {
    let repo = TestRepo::new().await;
    let g = repo.backend();
    let c1 = repo.head().await;
    repo.write("file.txt", "second\n");
    let c2 = repo.commit("second").await;
    assert_ne!(c1, c2);
    repo.write("file.txt", "uncommitted\n");

    g.reset_hard(&repo.root, &Sha::new(c1.as_str()))
        .await
        .expect("reset_hard");
    assert_eq!(repo.head().await, c1);
    assert_eq!(
        std::fs::read_to_string(repo.root.join("file.txt")).expect("read"),
        "base\n"
    );

    let err = g
        .reset_hard(&repo.root, &Sha::new(MISSING_SHA))
        .await
        .unwrap_err();
    assert!(
        matches!(err, GitError::NotFound(_)),
        "expected NotFound, got {err:?}"
    );
}

#[tokio::test]
async fn worktree_states_derives_detached_head() {
    let repo = TestRepo::new().await;
    let g = repo.backend();
    let base = repo.head().await;
    let wt = repo.wt_path("wt-det");
    g.create_worktree(&wt, "det", &Sha::new(base.as_str()))
        .await
        .expect("create_worktree");
    git_in(&wt, &["checkout", "--detach"]).await;

    let states = g.worktree_states().await.expect("worktree_states");
    assert_eq!(states.len(), 2, "main + one linked worktree, got {states:?}");

    // Match by final path component: git reports realpaths with `/`
    // separators on Windows, so exact-PathBuf equality is not portable here.
    let det = states
        .iter()
        .find(|s| s.path.ends_with("wt-det"))
        .expect("detached worktree entry");
    assert_eq!(det.branch, None, "detached HEAD must derive branch=None");
    assert_eq!(det.head.as_str(), base);
    assert_eq!(det.status, WorktreeStatus::Clean);

    let main_state = states
        .iter()
        .find(|s| s.branch.as_deref() == Some("main"))
        .expect("main worktree entry");
    assert_eq!(main_state.status, WorktreeStatus::Clean);
}
