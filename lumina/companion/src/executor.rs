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
//!   `.lumina/worktrees/<sanitised b>` (its committish `base` is resolved
//!   COMPANION-side via [`GitBackend::resolve_committish`] — the record-only
//!   server passes the string through verbatim);
//! - merges run in a dedicated INTEGRATION worktree at
//!   `.lumina/worktrees/integration-<sanitised target>`, lazily created as a
//!   DETACHED checkout of the target tip
//!   ([`GitBackend::attach_worktree_detached`]) on first use and re-pinned
//!   to the current tip ([`GitBackend::detach_worktree`]) on every reuse.
//!   The target BRANCH is never checked out here — git's one-checkout-per-
//!   branch rule therefore never collides with the operator's primary
//!   checkout, and the branch ref advances only via the post-merge
//!   compare-and-swap ([`GitBackend::update_branch_ref`]). The user's
//!   primary checkout at `repo_root` is never touched by a merge.
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
//! resolve the target tip FIRST ([`GitBackend::resolve_branch_tip`] — the
//! clean early `NotFound` and the CAS anchor, maximising the CAS-protected
//! window) -> observe [`GitBackend::worktree_states`] -> ensure a DETACHED
//! integration worktree at that tip: missing ->
//! [`GitBackend::attach_worktree_detached`]; present -> self-heal an
//! interrupted merge (`Conflicted` triggers [`GitBackend::abort_merge`]),
//! refuse a `Dirty` worktree ([`FailureKind::DirtyWorktree`]), THEN
//! [`GitBackend::detach_worktree`] at the tip. The abort-THEN-detach order
//! is load-bearing: an aborted LEGACY worktree lands back ON the target
//! branch, and `checkout --detach` (never a reset) is what migrates it to
//! the detached model without moving the branch ref. An executor-side
//! sanity check ([`GitBackend::head_of`] == resolved tip) replaces the old
//! in-merge checked-out-branch guard -> merge -> on conflict, abort LOCALLY
//! and report [`Outcome::Conflicted`] (§E: the resolve loop never crosses
//! the wire; no ref was touched) -> on success, run the SHA-stability gate
//! (ADR §H): every `must_remain_reachable` SHA must be an ancestor of the
//! new tip, else [`GitBackend::reset_hard`] back to the pre-merge tip and
//! report [`FailureKind::ReachabilityViolation`]. A gate check that ERRORS
//! (rather than answering `false`) also rolls back — an unverifiable merge
//! is never left in place. The gate runs BEFORE any ref moves, so a failed
//! gate never touched the real branch -> finally advance the branch ref via
//! the atomic compare-and-swap ([`GitBackend::update_branch_ref`], expected
//! old = the up-front resolved tip). A LOST CAS (the operator committed to
//! the target meanwhile, or deleted it) reports
//! [`FailureKind::TargetMoved`] with NO rollback: no ref was touched, the
//! orphan merge commit stays reflog-protected (~90 days), and the next run
//! re-detaches at the new tip. [`Outcome::Merged`] carries an optional
//! `target_checkout` operator hint derived from the PRE-MERGE
//! `worktree_states` snapshot: when some NON-integration worktree had the
//! target branch checked out, that checkout is now stale relative to the
//! advanced ref (remedy: `git reset --keep <merge_sha>` there).
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
//! are excluded; detached records — `branch: None` — pass through as-is) plus
//! `target_tip` = `head_of(repo_root)`. The default-target choice is
//! deliberate: the intent carries no branch parameter, so the primary
//! checkout's HEAD — the tip the server seeds sprints from — is the one
//! observable "integration target" tip. Revisit if Reconcile ever grows a
//! target parameter.
//!
//! ## GitError -> FailureKind mapping
//!
//! Centralised in [`fail`] / `kind_for`: `NotFound` -> `NotFound`,
//! `RefCasLost` -> `TargetMoved`, `Engine` -> `GitFailure`, `Invalid` | `Io`
//! -> `Internal`, and `State` -> a CALLER-SUPPLIED kind, because the seam
//! reuses `State` for several distinct situations (dirty worktree on remove
//! -> `DirtyWorktree`, existing branch on create -> `BranchInUse`); call
//! sites with no finer semantic pass `GitFailure`. `BranchInUse` is still
//! produced by the CREATE path; merges stopped producing it when the
//! integration worktree went detached.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lumina_protocol::{
    FailureKind, Intent, Outcome, Sha as WireSha, TargetCheckoutHint, WorktreeSnapshot,
};

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

    async fn create_worktree(&self, branch: &str, base: &str) -> Outcome {
        for (what, value) in [("branch", branch), ("base", base)] {
            if let Some(refused) = reject_leading_dash(what, value) {
                return refused;
            }
        }
        if let Err(e) = ensure_worktrees_excluded(&self.repo_root) {
            return exclude_failed(e);
        }
        // `base` is any committish string (the record-only server cannot
        // resolve refs); resolve it HERE, before any worktree is touched. An
        // unresolvable base maps straight to NotFound through the kind table.
        let base_sha = match self.backend.resolve_committish(base).await {
            Ok(sha) => sha,
            Err(e) => return fail(e, FailureKind::GitFailure),
        };
        let path = self.worktree_path_for_branch(branch);
        match self.backend.create_worktree(&path, branch, &base_sha).await {
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
        for (what, value) in [("source branch", source), ("target branch", target)] {
            if let Some(refused) = reject_leading_dash(what, value) {
                return refused;
            }
        }
        if let Err(e) = ensure_worktrees_excluded(&self.repo_root) {
            return exclude_failed(e);
        }
        let integration = self.integration_worktree_path(target);

        // (1): resolve the target tip FIRST — the clean early NotFound for a
        // missing target, the rollback anchor, and the CAS `expected_old`.
        // Everything after this line is CAS-protected: an operator commit to
        // the target between here and step (6) surfaces as TargetMoved. (The
        // server-side per-target merge lease serialises COMPANION-driven
        // merges; the CAS catches everyone else.)
        let expected_old_tip = match self.backend.resolve_branch_tip(target).await {
            Ok(tip) => tip,
            Err(e) => return fail(e, FailureKind::GitFailure),
        };

        // (2): one ground-truth observation drives ensure + self-heal + the
        // cleanliness refusal — and, at the end, the `target_checkout`
        // operator hint (deliberately the PRE-merge snapshot; staleness is
        // acceptable for a hint).
        let states = match self.backend.worktree_states().await {
            Ok(s) => s,
            Err(e) => return fail(e, FailureKind::GitFailure),
        };
        match states.iter().find(|s| same_path(&s.path, &integration)) {
            None => {
                // Lazily create the integration worktree as a DETACHED
                // checkout of the resolved tip. No branch is involved, so
                // git's "already checked out elsewhere" refusal cannot
                // trigger — the target being checked out in the operator's
                // primary checkout no longer blocks the merge.
                if let Err(e) = self
                    .backend
                    .attach_worktree_detached(&integration, expected_old_tip.as_str())
                    .await
                {
                    // No BranchInUse here by construction; an occupied path
                    // (an on-disk-but-unregistered leftover) is engine-level
                    // breakage. `State` has no finer meaning at this site
                    // than companion-internal.
                    return fail(e, FailureKind::Internal);
                }
            }
            Some(state) => {
                match state.status {
                    // Self-heal: an interrupted merge from a crashed prior
                    // run is aborted before this attempt proceeds. Our own
                    // invariant only ever merges in a clean worktree, so the
                    // post-abort state is the clean pre-merge one.
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
                }
                // Re-pin HEAD at the resolved tip — AFTER the abort above,
                // which is load-bearing: an aborted LEGACY worktree lands
                // back ON the target branch, and `checkout --detach` (never
                // a reset) migrates it off the branch without moving the
                // branch ref.
                if let Err(e) = self
                    .backend
                    .detach_worktree(&integration, expected_old_tip.as_str())
                    .await
                {
                    return fail(e, FailureKind::GitFailure);
                }
            }
        }

        // (3): executor-side sanity guard (replaces the old in-merge
        // checked-out-branch guard): the integration HEAD must sit exactly at
        // the resolved tip, else our invariants are broken and merging would
        // build on the wrong base.
        match self.backend.head_of(&integration).await {
            Ok(head) if head == expected_old_tip => {}
            Ok(head) => {
                return Outcome::Failed {
                    kind: FailureKind::Internal,
                    message: format!(
                        "integration worktree {} HEAD {head} is not at the resolved \
                         target tip {expected_old_tip}; refusing to merge",
                        integration.display()
                    ),
                };
            }
            Err(e) => return fail(e, FailureKind::GitFailure),
        }

        // (4): the merge itself, onto the detached HEAD.
        let result = match self.backend.merge(&integration, source, no_ff).await {
            Ok(r) => r,
            Err(e) => return fail(e, FailureKind::GitFailure),
        };
        let (new_tip, fast_forward) = match result {
            // HEAD pinned at the tip, so "unmoved" ⟺ the source is already
            // reachable from the target: no CAS needed, nothing changed.
            MergeResult::AlreadyUpToDate => {
                return Outcome::AlreadyUpToDate {
                    tip: wire_sha(expected_old_tip),
                };
            }
            // Coarse protocol — abort locally, report the terminal
            // Conflicted outcome. No resolve loop crosses the wire, and no
            // ref was touched.
            MergeResult::Conflict { paths } => {
                if let Err(e) = self.backend.abort_merge(&integration).await {
                    return fail(e, FailureKind::GitFailure);
                }
                return Outcome::Conflicted { paths };
            }
            MergeResult::FastForward { new_tip } => (new_tip, true),
            MergeResult::Merged { merge_sha } => (merge_sha, false),
        };

        // (5): the ADR §H SHA-stability gate, anchored on the resolved tip.
        // Violation OR an unverifiable check rolls the integration worktree
        // back. The gate runs BEFORE the CAS, so a failed gate never touched
        // the real branch.
        for sha in must_remain_reachable {
            match self.backend.is_ancestor(&git_sha(sha), &new_tip).await {
                Ok(true) => {}
                Ok(false) => {
                    return self
                        .rollback_failed(
                            &integration,
                            &expected_old_tip,
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
                            &expected_old_tip,
                            kind_for(&e, FailureKind::GitFailure),
                            format!("reachability check for {} failed: {e}", sha.0),
                        )
                        .await;
                }
            }
        }

        // (6): advance the branch ref with an atomic compare-and-swap. A
        // lost CAS maps to TargetMoved through the kind table, with NO
        // rollback: no ref was touched, the orphan merge commit stays
        // reflog-protected, and the next run re-detaches at the new tip.
        if let Err(e) = self
            .backend
            .update_branch_ref(
                target,
                &new_tip,
                &expected_old_tip,
                &format!("lumina-companion: merge {source}"),
            )
            .await
        {
            return fail(e, FailureKind::GitFailure);
        }

        // (7): the operator hint, derived from the pre-merge snapshot: was
        // the target branch checked out in some NON-integration worktree?
        // That checkout is now stale relative to the advanced ref.
        let target_checkout = states
            .iter()
            .find(|s| !same_path(&s.path, &integration) && s.branch.as_deref() == Some(target))
            .map(|s| TargetCheckoutHint {
                path: s.path.display().to_string(),
                dirty: s.status != WorktreeStatus::Clean,
            });

        Outcome::Merged {
            merge_sha: wire_sha(new_tip),
            fast_forward,
            target_checkout,
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
            .filter(|s| path_starts_with(&s.path, &managed_root))
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

/// Argument-injection guard at the executor's trust boundary: server-supplied
/// branch/committish strings reach git argv, and a value beginning with `-`
/// would parse as a git flag on the paths that cannot take an end-of-options
/// `--` separator (`checkout --detach`, `rev-parse --verify`). No valid git
/// ref can begin with `-`, so rejecting up front refuses nothing legitimate.
fn reject_leading_dash(what: &str, value: &str) -> Option<Outcome> {
    if value.starts_with('-') {
        return Some(Outcome::Failed {
            kind: FailureKind::NotFound,
            message: format!("{what} '{value}' is not a valid git ref: leading '-'"),
        });
    }
    None
}

/// Path identity robust to realpath drift: `git worktree list` reports
/// realpath-resolved paths while the executor constructs lexical ones, so a
/// symlinked repo root (macOS `/tmp` -> `/private/tmp`, Windows junctions) or
/// case drift would otherwise defeat the integration-worktree reuse match.
/// Compares canonicalised forms when BOTH sides canonicalise (on Windows both
/// gain the `\\?\` prefix, keeping the comparison consistent); falls back to
/// the lexical comparison when either side fails — the path may not exist yet
/// (e.g. the integration worktree before first creation, or fake-backend
/// paths in unit tests).
fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

/// Prefix variant of [`same_path`] for the managed-root containment check.
fn path_starts_with(path: &Path, base: &Path) -> bool {
    match (std::fs::canonicalize(path), std::fs::canonicalize(base)) {
        (Ok(cp), Ok(cb)) => cp.starts_with(cb),
        _ => path.starts_with(base),
    }
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
        GitError::RefCasLost(_) => FailureKind::TargetMoved,
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
