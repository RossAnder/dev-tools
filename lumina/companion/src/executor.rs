//! The intent -> outcome executor: one coarse [`Intent`] in, one coarse
//! [`Outcome`] back (ADR-0006 §E), driven entirely through the engine-neutral
//! [`GitBackend`] seam. This module is PURE LOGIC over that trait plus
//! `std::fs` for the `.git/info/exclude` registration — no transport, no
//! process spawning (the WS dial loop arrives in Task 6 and merely calls
//! [`Executor::execute`]).
//!
//! ## Translation boundary
//!
//! Per the protocol crate's contract, the companion translates
//! `GitBackend` <-> protocol types HERE and nowhere else: `git::Sha` <->
//! `protocol::Sha`, [`WorktreeState`] -> [`WorktreeSnapshot`],
//! [`GitError`] -> [`FailureKind`]. Neither vocabulary leaks across.
//!
//! ## Companion-managed worktrees (User Decision 1)
//!
//! Worktree layout is companion-owned. Every worktree the executor creates
//! lives under `<repo_root>/.lumina/worktrees/`:
//!
//! - [`Intent::CreateWorktree`] for branch `b` lands at
//!   `.lumina/worktrees/<sanitised b>`;
//! - merges run in a dedicated INTEGRATION worktree at
//!   `.lumina/worktrees/integration-<sanitised target>`, lazily attached to
//!   the EXISTING target branch ([`GitBackend::attach_worktree`]) on first
//!   use and reused when present and clean. The user's primary checkout at
//!   `repo_root` is never touched by a merge.
//!
//! The managed root is registered in `<repo_root>/.git/info/exclude`
//! (repo-local, never a tracked `.gitignore` edit) — append-only and
//! idempotent, via [`ensure_worktrees_excluded`]. `repo_root` must be the
//! PRIMARY checkout (where `.git` is a directory); a linked worktree's
//! `.git` FILE makes the registration fail cleanly as
//! [`FailureKind::Internal`].
//!
//! ## Merge choreography ([`Intent::MergeWorktree`])
//!
//! ensure integration worktree -> self-heal an interrupted merge (a
//! `Conflicted` status from [`GitBackend::worktree_states`] triggers
//! [`GitBackend::abort_merge`]) -> refuse a `Dirty` worktree
//! ([`FailureKind::DirtyWorktree`], keeping a later abort deterministic) ->
//! record the pre-merge tip -> merge -> on conflict, abort LOCALLY and report
//! [`Outcome::Conflicted`] (§E: the resolve loop never crosses the wire) ->
//! on success, run the SHA-stability gate (ADR §H): every
//! `must_remain_reachable` SHA must be an ancestor of the new tip, else
//! [`GitBackend::reset_hard`] back to the pre-merge tip and report
//! [`FailureKind::ReachabilityViolation`]. A gate check that ERRORS (rather
//! than answering `false`) also rolls back — an unverifiable merge is never
//! left in place.
//!
//! Residual self-heal gap (documented choice): an interrupted merge whose
//! conflicts were all staged shows as `Dirty` (not `Conflicted`) under the
//! frozen seam's status taxonomy, so it is REFUSED as a dirty worktree rather
//! than healed — deterministic and safe; an operator (or a future seam
//! widening) clears it.
//!
//! ## Reconcile
//!
//! [`Intent::Reconcile`] reports the worktrees under the managed root (the
//! ones the companion owns — the primary checkout and any user-made worktrees
//! are excluded) plus `target_tip` = `head_of(repo_root)`. The default-target
//! choice is deliberate: the intent carries no branch parameter and the
//! frozen seam has no branch-tip query, so the primary checkout's HEAD — the
//! tip the server seeds sprints from — is the one observable "integration
//! target" tip. Revisit if Reconcile ever grows a target parameter.
//!
//! ## GitError -> FailureKind mapping
//!
//! Centralised in [`fail`] / `kind_for`: `NotFound` -> `NotFound`, `Engine`
//! -> `GitFailure`, `Invalid` | `Io` -> `Internal`, and `State` -> a
//! CALLER-SUPPLIED kind, because the seam reuses `State` for several distinct
//! situations (dirty worktree on remove -> `DirtyWorktree`, existing branch
//! on create / branch checked out elsewhere on attach -> `BranchInUse`); call
//! sites with no finer semantic pass `GitFailure`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lumina_protocol::{FailureKind, Intent, Outcome, Sha as WireSha, WorktreeSnapshot};

use crate::git::{GitBackend, GitError, MergeResult, Sha as GitSha, WorktreeState, WorktreeStatus};

/// The line registered in `.git/info/exclude` to hide the managed worktrees
/// (see [`Executor::managed_worktrees_root`]) from the user's `git status`
/// (anchored, directory-only).
pub const WORKTREES_EXCLUDE_ENTRY: &str = "/.lumina/worktrees/";

/// Maps one [`Intent`] to one [`Outcome`] over an erased [`GitBackend`].
///
/// `Arc<dyn GitBackend>` (rather than `Box`) so the Task-6 connection loop
/// can share one backend between the executor and any concurrent observer,
/// and so tests keep a concrete `Arc<FakeGitBackend>` handle for call-log
/// assertions.
pub struct Executor {
    repo_root: PathBuf,
    backend: Arc<dyn GitBackend>,
}

impl Executor {
    /// An executor for the repository rooted at `repo_root` (the PRIMARY
    /// checkout), driving `backend`.
    pub fn new(repo_root: impl Into<PathBuf>, backend: Arc<dyn GitBackend>) -> Self {
        Self {
            repo_root: repo_root.into(),
            backend,
        }
    }

    /// The directory all companion-managed worktrees live under.
    pub fn managed_worktrees_root(&self) -> PathBuf {
        self.repo_root.join(".lumina").join("worktrees")
    }

    /// Where [`Intent::CreateWorktree`] puts the worktree for `branch`.
    pub fn worktree_path_for_branch(&self, branch: &str) -> PathBuf {
        self.managed_worktrees_root().join(branch_dir_name(branch))
    }

    /// Where merges into `target_branch` run (User Decision 1): the dedicated
    /// integration worktree for that target.
    pub fn integration_worktree_path(&self, target_branch: &str) -> PathBuf {
        self.managed_worktrees_root()
            .join(format!("integration-{}", branch_dir_name(target_branch)))
    }

    /// Execute one intent to its single coarse outcome. Infallible by
    /// construction: every error becomes [`Outcome::Failed`].
    pub async fn execute(&self, intent: Intent) -> Outcome {
        match intent {
            Intent::CreateWorktree { branch, base } => self.create_worktree(&branch, &base).await,
            Intent::RemoveWorktree { path, force } => self.remove_worktree(&path, force).await,
            Intent::CommitCheckpoint { path, message } => {
                self.commit_checkpoint(&path, &message).await
            }
            Intent::MergeWorktree {
                source_branch,
                target_branch,
                must_remain_reachable,
                no_ff,
            } => {
                self.merge_worktree(&source_branch, &target_branch, &must_remain_reachable, no_ff)
                    .await
            }
            Intent::Reconcile => self.reconcile().await,
        }
    }

    async fn create_worktree(&self, branch: &str, base: &WireSha) -> Outcome {
        if let Err(e) = ensure_worktrees_excluded(&self.repo_root) {
            return exclude_failed(e);
        }
        let path = self.worktree_path_for_branch(branch);
        match self
            .backend
            .create_worktree(&path, branch, &git_sha(base))
            .await
        {
            // `State` here means the branch (or path) already exists / is in
            // use — the closest wire category is BranchInUse.
            Err(e) => fail(e, FailureKind::BranchInUse),
            Ok(state) => Outcome::WorktreeCreated {
                path: state.path.display().to_string(),
                branch: branch.to_owned(),
                head: wire_sha(state.head),
            },
        }
    }

    async fn remove_worktree(&self, path: &str, force: bool) -> Outcome {
        match self.backend.remove_worktree(Path::new(path), force).await {
            Ok(()) => Outcome::WorktreeRemoved,
            // `State` here means uncommitted changes without `force`.
            Err(e) => fail(e, FailureKind::DirtyWorktree),
        }
    }

    async fn commit_checkpoint(&self, path: &str, message: &str) -> Outcome {
        // A checkpoint is commit-all by protocol contract: stage everything,
        // commit; selective staging never crosses the wire in v1.
        let worktree = Path::new(path);
        match self.backend.commit_all(worktree, message).await {
            Ok(Some(sha)) => Outcome::Checkpointed {
                commit_sha: wire_sha(sha),
            },
            // Nothing to commit: the idempotent-re-run outcome, reported with
            // the worktree's unchanged HEAD per the protocol contract.
            Ok(None) => match self.backend.head_of(worktree).await {
                Ok(tip) => Outcome::AlreadyUpToDate { tip: wire_sha(tip) },
                Err(e) => fail(e, FailureKind::GitFailure),
            },
            Err(e) => fail(e, FailureKind::GitFailure),
        }
    }

    async fn merge_worktree(
        &self,
        source: &str,
        target: &str,
        must_remain_reachable: &[WireSha],
        no_ff: bool,
    ) -> Outcome {
        if let Err(e) = ensure_worktrees_excluded(&self.repo_root) {
            return exclude_failed(e);
        }
        let integration = self.integration_worktree_path(target);

        // (1)-(3): one ground-truth observation drives ensure + self-heal +
        // the cleanliness refusal.
        let states = match self.backend.worktree_states().await {
            Ok(s) => s,
            Err(e) => return fail(e, FailureKind::GitFailure),
        };
        match states.iter().find(|s| s.path == integration) {
            None => {
                // Lazily attach the integration worktree to the EXISTING
                // target branch. A missing target branch surfaces naturally
                // as `NotFound`; `State` means the target is checked out in
                // another worktree (or the path is occupied) — BranchInUse is
                // the closest wire category.
                if let Err(e) = self.backend.attach_worktree(target, &integration).await {
                    return fail(e, FailureKind::BranchInUse);
                }
            }
            Some(state) => match state.status {
                // Self-heal: an interrupted merge from a crashed prior run is
                // aborted before this attempt proceeds. Our own invariant only
                // ever merges in a clean worktree, so the post-abort state is
                // the clean pre-merge one.
                WorktreeStatus::Conflicted => {
                    if let Err(e) = self.backend.abort_merge(&integration).await {
                        return fail(e, FailureKind::GitFailure);
                    }
                }
                WorktreeStatus::Dirty => {
                    return Outcome::Failed {
                        kind: FailureKind::DirtyWorktree,
                        message: format!(
                            "integration worktree {} has uncommitted changes; refusing to merge",
                            integration.display()
                        ),
                    };
                }
                WorktreeStatus::Clean => {}
            },
        }

        // (4): the rollback anchor.
        let pre_tip = match self.backend.head_of(&integration).await {
            Ok(tip) => tip,
            Err(e) => return fail(e, FailureKind::GitFailure),
        };

        // (5): the merge itself. `State` here means the wrong branch is
        // checked out in the integration worktree — an internal invariant
        // breach with no finer wire category.
        let result = match self.backend.merge(&integration, source, target, no_ff).await {
            Ok(r) => r,
            Err(e) => return fail(e, FailureKind::GitFailure),
        };
        let (new_tip, fast_forward) = match result {
            MergeResult::AlreadyUpToDate => {
                return Outcome::AlreadyUpToDate {
                    tip: wire_sha(pre_tip),
                };
            }
            // (6): coarse protocol — abort locally, report the terminal
            // Conflicted outcome. No resolve loop crosses the wire.
            MergeResult::Conflict { paths } => {
                if let Err(e) = self.backend.abort_merge(&integration).await {
                    return fail(e, FailureKind::GitFailure);
                }
                return Outcome::Conflicted { paths };
            }
            MergeResult::FastForward { new_tip } => (new_tip, true),
            MergeResult::Merged { merge_sha } => (merge_sha, false),
        };

        // (7): the ADR §H SHA-stability gate. Violation OR an unverifiable
        // check rolls the integration worktree back to the pre-merge tip.
        for sha in must_remain_reachable {
            match self.backend.is_ancestor(&git_sha(sha), &new_tip).await {
                Ok(true) => {}
                Ok(false) => {
                    return self
                        .rollback_failed(
                            &integration,
                            &pre_tip,
                            FailureKind::ReachabilityViolation,
                            format!(
                                "merge of `{source}` into `{target}` left {} unreachable \
                                 from new tip {new_tip}",
                                sha.0
                            ),
                        )
                        .await;
                }
                Err(e) => {
                    return self
                        .rollback_failed(
                            &integration,
                            &pre_tip,
                            kind_for(&e, FailureKind::GitFailure),
                            format!("reachability check for {} failed: {e}", sha.0),
                        )
                        .await;
                }
            }
        }

        Outcome::Merged {
            merge_sha: wire_sha(new_tip),
            fast_forward,
        }
    }

    /// Roll the integration worktree back to `pre_tip`, then report `kind` /
    /// `message`. A FAILED rollback escalates to `Internal` with both errors
    /// in the message — the worktree is in an unexpected state and a human
    /// (or Reconcile) must look.
    async fn rollback_failed(
        &self,
        worktree: &Path,
        pre_tip: &GitSha,
        kind: FailureKind,
        message: String,
    ) -> Outcome {
        match self.backend.reset_hard(worktree, pre_tip).await {
            Ok(()) => Outcome::Failed {
                kind,
                message: format!("{message}; rolled back to pre-merge tip {pre_tip}"),
            },
            Err(reset_err) => Outcome::Failed {
                kind: FailureKind::Internal,
                message: format!(
                    "{message}; ADDITIONALLY the rollback to pre-merge tip {pre_tip} \
                     failed: {reset_err}"
                ),
            },
        }
    }

    async fn reconcile(&self) -> Outcome {
        let states = match self.backend.worktree_states().await {
            Ok(s) => s,
            Err(e) => return fail(e, FailureKind::GitFailure),
        };
        // See the module doc: target_tip = the primary checkout's HEAD.
        let target_tip = match self.backend.head_of(&self.repo_root).await {
            Ok(tip) => tip,
            Err(e) => return fail(e, FailureKind::GitFailure),
        };
        let managed_root = self.managed_worktrees_root();
        let worktrees = states
            .into_iter()
            .filter(|s| s.path.starts_with(&managed_root))
            .map(snapshot)
            .collect();
        Outcome::Reconciled {
            worktrees,
            target_tip: wire_sha(target_tip),
        }
    }
}

/// Register [`WORKTREES_EXCLUDE_ENTRY`] in `<repo_root>/.git/info/exclude` so
/// the companion-managed worktrees never show up in the user's `git status`.
/// Append-only and idempotent: an already-present entry (whitespace-trimmed
/// line match) is left alone, and existing content is never rewritten. The
/// `info/` directory is created if absent; a `repo_root` whose `.git` is a
/// FILE (a linked worktree) fails with the underlying io error.
pub fn ensure_worktrees_excluded(repo_root: &Path) -> std::io::Result<()> {
    let info_dir = repo_root.join(".git").join("info");
    std::fs::create_dir_all(&info_dir)?;
    let exclude = info_dir.join("exclude");
    let existing = match std::fs::read_to_string(&exclude) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    if existing
        .lines()
        .any(|line| line.trim() == WORKTREES_EXCLUDE_ENTRY)
    {
        return Ok(());
    }
    let mut entry = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        entry.push('\n');
    }
    entry.push_str(WORKTREES_EXCLUDE_ENTRY);
    entry.push('\n');
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude)?;
    file.write_all(entry.as_bytes())
}

/// A filesystem-safe directory name for `branch`: every char outside
/// `[A-Za-z0-9._-]` (notably `/` in hierarchical branch names) becomes `-`.
fn branch_dir_name(branch: &str) -> String {
    branch
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// The [`GitError`] -> [`FailureKind`] table (see the module doc). `State` is
/// context-dependent, so the caller supplies its meaning at that call site.
fn kind_for(err: &GitError, state_kind: FailureKind) -> FailureKind {
    match err {
        GitError::NotFound(_) => FailureKind::NotFound,
        GitError::State(_) => state_kind,
        GitError::Engine(_) => FailureKind::GitFailure,
        GitError::Invalid(_) | GitError::Io(_) => FailureKind::Internal,
    }
}

/// Render `err` as the wire [`Outcome::Failed`], with `State` mapped to the
/// caller-supplied `state_kind` per [`kind_for`].
fn fail(err: GitError, state_kind: FailureKind) -> Outcome {
    Outcome::Failed {
        kind: kind_for(&err, state_kind),
        message: err.to_string(),
    }
}

/// An `.git/info/exclude` registration failure — companion-internal, not a
/// git verdict.
fn exclude_failed(err: std::io::Error) -> Outcome {
    Outcome::Failed {
        kind: FailureKind::Internal,
        message: format!("registering .git/info/exclude entry: {err}"),
    }
}

fn git_sha(sha: &WireSha) -> GitSha {
    GitSha::new(sha.0.clone())
}

fn wire_sha(sha: GitSha) -> WireSha {
    WireSha(sha.into_string())
}

fn snapshot(state: WorktreeState) -> WorktreeSnapshot {
    WorktreeSnapshot {
        path: state.path.display().to_string(),
        branch: state.branch,
        head: wire_sha(state.head),
        // The wire shape is a plain dirty flag; Conflicted is a (very) dirty
        // worktree from the server's perspective.
        dirty: state.status != WorktreeStatus::Clean,
    }
}
