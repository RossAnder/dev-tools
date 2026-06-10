//! [`FakeGitBackend`] — a scripted, in-memory [`GitBackend`].
//!
//! Two jobs: (1) the seam's NEUTRALITY PROOF — it implements the full trait
//! with zero process/porcelain types, so the trait demonstrably does not
//! depend on shell-git; (2) the executor's test double (Task 4). Each method
//! pops the next scripted response off its own FIFO queue and records the
//! call (with its arguments) into a shared log; an UNSCRIPTED call panics
//! with the method name so a test's missing script surfaces immediately
//! rather than as a misleading downstream assertion.
//!
//! Not `#[cfg(test)]`-gated on purpose: the executor's tests (and any future
//! integration test) consume it from outside this module.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;

use super::{
    GitBackend, GitError, MergeResult, ResolveOp, ResolveOutcome, Sha, WorktreeState,
};

/// One recorded [`GitBackend`] call, with the arguments the caller passed —
/// the executor's tests assert against this log.
#[derive(Debug, Clone, PartialEq)]
pub enum FakeCall {
    CreateWorktree {
        path: PathBuf,
        branch: String,
        start_point: Sha,
    },
    RemoveWorktree {
        path: PathBuf,
        force: bool,
    },
    CommitAll {
        worktree: PathBuf,
        message: String,
    },
    Merge {
        worktree: PathBuf,
        source: String,
        target: String,
        no_ff: bool,
    },
    AbortMerge {
        worktree: PathBuf,
    },
    Resolve {
        worktree: PathBuf,
        op: ResolveOp,
    },
    IsAncestor {
        ancestor: Sha,
        descendant: Sha,
    },
    CommitExists {
        sha: Sha,
    },
    WorktreeStates,
    HeadOf {
        worktree: PathBuf,
    },
    ResetHard {
        worktree: PathBuf,
        to: Sha,
    },
}

/// Scripted in-memory backend: one FIFO response queue per trait method plus
/// a shared call log. Interior mutability via `std::sync::Mutex` — the locks
/// are never held across an `.await` (every method is synchronous inside).
#[derive(Default)]
pub struct FakeGitBackend {
    calls: Mutex<Vec<FakeCall>>,
    create_worktree: Mutex<VecDeque<Result<WorktreeState, GitError>>>,
    remove_worktree: Mutex<VecDeque<Result<(), GitError>>>,
    commit_all: Mutex<VecDeque<Result<Option<Sha>, GitError>>>,
    merge: Mutex<VecDeque<Result<MergeResult, GitError>>>,
    abort_merge: Mutex<VecDeque<Result<(), GitError>>>,
    resolve: Mutex<VecDeque<Result<ResolveOutcome, GitError>>>,
    is_ancestor: Mutex<VecDeque<Result<bool, GitError>>>,
    commit_exists: Mutex<VecDeque<Result<bool, GitError>>>,
    worktree_states: Mutex<VecDeque<Result<Vec<WorktreeState>, GitError>>>,
    head_of: Mutex<VecDeque<Result<Sha, GitError>>>,
    reset_hard: Mutex<VecDeque<Result<(), GitError>>>,
}

/// Pop the next scripted response or panic with the offending method name.
fn pop<T>(queue: &Mutex<VecDeque<T>>, method: &str) -> T {
    queue
        .lock()
        .expect("FakeGitBackend queue mutex poisoned")
        .pop_front()
        .unwrap_or_else(|| panic!("FakeGitBackend: no scripted response left for `{method}`"))
}

impl FakeGitBackend {
    /// An empty fake: every method panics until a response is scripted.
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot the calls recorded so far, in invocation order.
    pub fn calls(&self) -> Vec<FakeCall> {
        self.calls
            .lock()
            .expect("FakeGitBackend call-log mutex poisoned")
            .clone()
    }

    fn record(&self, call: FakeCall) {
        self.calls
            .lock()
            .expect("FakeGitBackend call-log mutex poisoned")
            .push(call);
    }

    // --- per-method scripting (FIFO: first pushed = first returned) ---

    pub fn push_create_worktree(&self, r: Result<WorktreeState, GitError>) {
        self.create_worktree.lock().unwrap().push_back(r);
    }

    pub fn push_remove_worktree(&self, r: Result<(), GitError>) {
        self.remove_worktree.lock().unwrap().push_back(r);
    }

    pub fn push_commit_all(&self, r: Result<Option<Sha>, GitError>) {
        self.commit_all.lock().unwrap().push_back(r);
    }

    pub fn push_merge(&self, r: Result<MergeResult, GitError>) {
        self.merge.lock().unwrap().push_back(r);
    }

    pub fn push_abort_merge(&self, r: Result<(), GitError>) {
        self.abort_merge.lock().unwrap().push_back(r);
    }

    pub fn push_resolve(&self, r: Result<ResolveOutcome, GitError>) {
        self.resolve.lock().unwrap().push_back(r);
    }

    pub fn push_is_ancestor(&self, r: Result<bool, GitError>) {
        self.is_ancestor.lock().unwrap().push_back(r);
    }

    pub fn push_commit_exists(&self, r: Result<bool, GitError>) {
        self.commit_exists.lock().unwrap().push_back(r);
    }

    pub fn push_worktree_states(&self, r: Result<Vec<WorktreeState>, GitError>) {
        self.worktree_states.lock().unwrap().push_back(r);
    }

    pub fn push_head_of(&self, r: Result<Sha, GitError>) {
        self.head_of.lock().unwrap().push_back(r);
    }

    pub fn push_reset_hard(&self, r: Result<(), GitError>) {
        self.reset_hard.lock().unwrap().push_back(r);
    }
}

#[async_trait]
impl GitBackend for FakeGitBackend {
    async fn create_worktree(
        &self,
        path: &Path,
        branch: &str,
        start_point: &Sha,
    ) -> Result<WorktreeState, GitError> {
        self.record(FakeCall::CreateWorktree {
            path: path.to_path_buf(),
            branch: branch.to_owned(),
            start_point: start_point.clone(),
        });
        pop(&self.create_worktree, "create_worktree")
    }

    async fn remove_worktree(&self, path: &Path, force: bool) -> Result<(), GitError> {
        self.record(FakeCall::RemoveWorktree {
            path: path.to_path_buf(),
            force,
        });
        pop(&self.remove_worktree, "remove_worktree")
    }

    async fn commit_all(&self, worktree: &Path, message: &str) -> Result<Option<Sha>, GitError> {
        self.record(FakeCall::CommitAll {
            worktree: worktree.to_path_buf(),
            message: message.to_owned(),
        });
        pop(&self.commit_all, "commit_all")
    }

    async fn merge(
        &self,
        worktree: &Path,
        source: &str,
        target: &str,
        no_ff: bool,
    ) -> Result<MergeResult, GitError> {
        self.record(FakeCall::Merge {
            worktree: worktree.to_path_buf(),
            source: source.to_owned(),
            target: target.to_owned(),
            no_ff,
        });
        pop(&self.merge, "merge")
    }

    async fn abort_merge(&self, worktree: &Path) -> Result<(), GitError> {
        self.record(FakeCall::AbortMerge {
            worktree: worktree.to_path_buf(),
        });
        pop(&self.abort_merge, "abort_merge")
    }

    async fn resolve(&self, worktree: &Path, op: ResolveOp) -> Result<ResolveOutcome, GitError> {
        self.record(FakeCall::Resolve {
            worktree: worktree.to_path_buf(),
            op,
        });
        pop(&self.resolve, "resolve")
    }

    async fn is_ancestor(&self, ancestor: &Sha, descendant: &Sha) -> Result<bool, GitError> {
        self.record(FakeCall::IsAncestor {
            ancestor: ancestor.clone(),
            descendant: descendant.clone(),
        });
        pop(&self.is_ancestor, "is_ancestor")
    }

    async fn commit_exists(&self, sha: &Sha) -> Result<bool, GitError> {
        self.record(FakeCall::CommitExists { sha: sha.clone() });
        pop(&self.commit_exists, "commit_exists")
    }

    async fn worktree_states(&self) -> Result<Vec<WorktreeState>, GitError> {
        self.record(FakeCall::WorktreeStates);
        pop(&self.worktree_states, "worktree_states")
    }

    async fn head_of(&self, worktree: &Path) -> Result<Sha, GitError> {
        self.record(FakeCall::HeadOf {
            worktree: worktree.to_path_buf(),
        });
        pop(&self.head_of, "head_of")
    }

    async fn reset_hard(&self, worktree: &Path, to: &Sha) -> Result<(), GitError> {
        self.record(FakeCall::ResetHard {
            worktree: worktree.to_path_buf(),
            to: to.clone(),
        });
        pop(&self.reset_hard, "reset_hard")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::WorktreeStatus;
    use super::*;

    fn wt_state(path: &str, branch: Option<&str>, head: &str, status: WorktreeStatus) -> WorktreeState {
        WorktreeState {
            path: PathBuf::from(path),
            branch: branch.map(str::to_owned),
            head: Sha::new(head),
            status,
        }
    }

    /// The object-safety proof: the fake drives the FULL trait surface through
    /// `Box<dyn GitBackend>` — the exact shape the executor consumes.
    #[tokio::test]
    async fn full_surface_works_behind_a_box() {
        let fake = FakeGitBackend::new();
        fake.push_create_worktree(Ok(wt_state(
            "/wt/sprint-1",
            Some("sprint-1"),
            "base-1",
            WorktreeStatus::Clean,
        )));
        fake.push_commit_all(Ok(Some(Sha::new("commit-1"))));
        fake.push_merge(Ok(MergeResult::Merged {
            merge_sha: Sha::new("merge-1"),
        }));
        fake.push_is_ancestor(Ok(true));
        fake.push_commit_exists(Ok(true));
        fake.push_worktree_states(Ok(vec![wt_state(
            "/repo",
            Some("main"),
            "merge-1",
            WorktreeStatus::Clean,
        )]));
        fake.push_head_of(Ok(Sha::new("merge-1")));
        fake.push_resolve(Ok(ResolveOutcome::Aborted));
        fake.push_abort_merge(Ok(()));
        fake.push_reset_hard(Ok(()));
        fake.push_remove_worktree(Ok(()));

        let backend: Box<dyn GitBackend> = Box::new(fake);
        let wt = Path::new("/wt/sprint-1");

        let created = backend
            .create_worktree(wt, "sprint-1", &Sha::new("base-1"))
            .await
            .unwrap();
        assert_eq!(created.branch.as_deref(), Some("sprint-1"));
        assert_eq!(
            backend.commit_all(wt, "checkpoint").await.unwrap(),
            Some(Sha::new("commit-1"))
        );
        assert_eq!(
            backend.merge(wt, "sprint-1", "main", true).await.unwrap(),
            MergeResult::Merged {
                merge_sha: Sha::new("merge-1")
            }
        );
        assert!(
            backend
                .is_ancestor(&Sha::new("commit-1"), &Sha::new("merge-1"))
                .await
                .unwrap()
        );
        assert!(backend.commit_exists(&Sha::new("merge-1")).await.unwrap());
        assert_eq!(backend.worktree_states().await.unwrap().len(), 1);
        assert_eq!(backend.head_of(wt).await.unwrap(), Sha::new("merge-1"));
        assert_eq!(
            backend.resolve(wt, ResolveOp::Abort).await.unwrap(),
            ResolveOutcome::Aborted
        );
        backend.abort_merge(wt).await.unwrap();
        backend.reset_hard(wt, &Sha::new("base-1")).await.unwrap();
        backend.remove_worktree(wt, false).await.unwrap();
    }

    /// Scripted responses pop FIFO: two queued merge outcomes come back in
    /// push order.
    #[tokio::test]
    async fn scripted_responses_pop_in_fifo_order() {
        let fake = FakeGitBackend::new();
        fake.push_merge(Ok(MergeResult::AlreadyUpToDate));
        fake.push_merge(Ok(MergeResult::Conflict {
            paths: vec!["src/a.rs".to_owned()],
        }));

        let backend: Box<dyn GitBackend> = Box::new(fake);
        let wt = Path::new("/wt");
        assert_eq!(
            backend.merge(wt, "feature", "main", false).await.unwrap(),
            MergeResult::AlreadyUpToDate
        );
        assert_eq!(
            backend.merge(wt, "feature", "main", false).await.unwrap(),
            MergeResult::Conflict {
                paths: vec!["src/a.rs".to_owned()]
            }
        );
    }

    /// Scripted errors flow through unchanged (here via `Arc<dyn GitBackend>`,
    /// the shared-ownership consumption shape, which also lets the test keep a
    /// concrete handle for the call-log assertion).
    #[tokio::test]
    async fn errors_and_call_log_round_trip() {
        let fake = Arc::new(FakeGitBackend::new());
        fake.push_abort_merge(Err(GitError::State("no merge in progress".to_owned())));

        let backend: Arc<dyn GitBackend> = fake.clone();
        let err = backend.abort_merge(Path::new("/wt")).await.unwrap_err();
        assert!(matches!(err, GitError::State(m) if m == "no merge in progress"));

        assert_eq!(
            fake.calls(),
            vec![FakeCall::AbortMerge {
                worktree: PathBuf::from("/wt"),
            }]
        );
    }

    /// An unscripted call panics with the method name, so a missing script
    /// surfaces at the call site rather than as a downstream assertion.
    #[tokio::test]
    #[should_panic(expected = "no scripted response left for `head_of`")]
    async fn unscripted_call_panics_with_method_name() {
        let fake = FakeGitBackend::new();
        let backend: Box<dyn GitBackend> = Box::new(fake);
        let _ = backend.head_of(Path::new("/wt")).await;
    }
}
