//! The engine-neutral git seam: [`GitBackend`] and the value types it speaks.
//!
//! Modelled on `lumina-core`'s `DbClient`/`DbTx` erased seam: the trait is
//! **object-safe** (no generic methods — the executor consumes it as
//! `Box<dyn GitBackend>`), and every public shape is engine-neutral — no
//! `std::process::Output`, exit codes, or porcelain structures leak through —
//! so a future gitoxide/libgit2 backend can implement the same surface without
//! a signature change. `ShellGit` (Task 3) is the shell-git v1 implementation;
//! [`fake::FakeGitBackend`] is the neutrality proof and the executor's test
//! double. ShellGit-only affordances (notably `run_git`) are inherent methods
//! on the concrete type, quarantined OFF this migratable trait surface.
//!
//! ## Surface freeze
//! The trait below is FROZEN after Task 2: ShellGit (Task 3) and the executor
//! (Task 4) build against it in parallel next wave. Do not add, remove, or
//! re-sign methods without coordinating both. The surface was widened ONCE,
//! between waves, as a coordinated change: [`GitBackend::attach_worktree`]
//! was added so the executor can attach the integration worktree to the
//! EXISTING target branch (ADR-0006 User Decision 1) — every pre-existing
//! item is byte-compatible with the frozen Task-2 shape.
//!
//! ## Addressing model
//! A backend instance fronts ONE repository (constructed per repo root; all
//! linked worktrees share that object DB). Methods that act *inside* a
//! particular checkout take an explicit `worktree: &Path` (ShellGit maps this
//! to `git -C <worktree>`); object-DB-scoped queries ([`GitBackend::is_ancestor`],
//! [`GitBackend::commit_exists`], [`GitBackend::worktree_states`]) take no
//! worktree and run against the repo root.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

pub mod fake;
pub use fake::FakeGitBackend;
pub mod shell;
pub use shell::ShellGit;

/// An opaque commit id. Deliberately UNVALIDATED (no hex/length check): the
/// engine mints real values via rev-parse, and test doubles may use readable
/// ids (`Sha::new("fake-tip-1")`). Treat it as an identity token, never parse
/// its contents.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sha(String);

impl Sha {
    /// Wrap a commit id.
    pub fn new(sha: impl Into<String>) -> Self {
        Sha(sha.into())
    }

    /// Borrow the id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Unwrap into the owned id string.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for Sha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for Sha {
    fn from(s: String) -> Self {
        Sha(s)
    }
}

impl From<&str> for Sha {
    fn from(s: &str) -> Self {
        Sha(s.to_owned())
    }
}

impl AsRef<str> for Sha {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// The dirtiness classification of one checkout (the Reconcile discriminant).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeStatus {
    /// No staged, unstaged, or untracked changes.
    Clean,
    /// Uncommitted (staged / unstaged / untracked) changes present, but no
    /// merge in progress.
    Dirty,
    /// A merge is in progress with unresolved conflict entries (under shell
    /// git: `MERGE_HEAD` present + `ls-files -u` non-empty).
    Conflicted,
}

/// One checkout's observed state, as returned by
/// [`GitBackend::worktree_states`] — the Reconcile input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeState {
    /// Absolute path of the checkout.
    pub path: PathBuf,
    /// The checked-out branch; `None` = detached HEAD.
    pub branch: Option<String>,
    /// The commit `HEAD` points at.
    pub head: Sha,
    /// Clean / dirty / conflicted classification.
    pub status: WorktreeStatus,
}

/// The outcome of [`GitBackend::merge`]. A conflict is a *normal outcome*
/// (the executor proceeds to [`GitBackend::resolve`] /
/// [`GitBackend::abort_merge`]), NOT a [`GitError`].
///
/// Already-up-to-date is modelled as a fourth VARIANT rather than a distinct
/// return channel: the executor then matches ONE exhaustive enum per merge
/// attempt, and "nothing to do" is just another outcome of the same operation
/// (it maps to a no-op record on the control plane, exactly like the other
/// arms map to their records).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeResult {
    /// The target branch moved forward to `new_tip`; no merge commit created.
    FastForward { new_tip: Sha },
    /// A merge commit was created.
    Merged { merge_sha: Sha },
    /// Conflicts at `paths` (repo-relative); the merge is left in progress in
    /// the worktree, awaiting [`GitBackend::resolve`] or
    /// [`GitBackend::abort_merge`].
    Conflict { paths: Vec<String> },
    /// `source` was already reachable from the target; nothing changed.
    AlreadyUpToDate,
}

/// One semantic conflict-resolution step, applied by [`GitBackend::resolve`]
/// to an in-progress merge. Path lists are repo-relative; an EMPTY list means
/// "every currently-conflicted path".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveOp {
    /// Resolve the named paths by taking the target side's content.
    TakeOurs { paths: Vec<String> },
    /// Resolve the named paths by taking the source side's content.
    TakeTheirs { paths: Vec<String> },
    /// Mark the named paths resolved AS THEY SIT ON DISK (stage them) — the
    /// arm for content someone edited by hand between steps.
    StageResolution { paths: Vec<String> },
    /// Complete the in-progress merge, committing the staged resolution.
    Continue,
    /// Abandon the merge and restore the pre-merge HEAD. Semantically the
    /// same operation as [`GitBackend::abort_merge`]; duplicated here so a
    /// scripted resolution SEQUENCE can end in an abort without switching
    /// call paths.
    Abort,
}

/// What a [`GitBackend::resolve`] step left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveOutcome {
    /// The merge completed; `merge_sha` is the resulting merge commit.
    Completed { merge_sha: Sha },
    /// The merge was abandoned; the worktree is back at the pre-merge HEAD.
    Aborted,
    /// Conflicts remain at `remaining` (repo-relative); the merge is still in
    /// progress.
    Pending { remaining: Vec<String> },
}

/// The neutral error taxonomy for [`GitBackend`] operations.
///
/// Mirrors `lumina-core`'s `AppError` discipline: variants carry pre-rendered,
/// caller-facing `String` messages — never `std::process::Output`, exit
/// codes, or porcelain structures — so the taxonomy survives an engine swap
/// untouched. A merge CONFLICT is deliberately NOT an error (it is
/// [`MergeResult::Conflict`] / [`ResolveOutcome::Pending`]): an error here
/// means the operation itself could not run or finish.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// A named ref / commit / worktree does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// Caller input rejected before touching the engine (e.g. an illegal
    /// branch name, a worktree path outside the repository).
    #[error("invalid: {0}")]
    Invalid(String),
    /// The repository or worktree is in the wrong state for the operation
    /// (e.g. `resolve`/`abort_merge` with no merge in progress; a dirty
    /// worktree where the operation requires clean; `merge` with the wrong
    /// branch checked out).
    #[error("state: {0}")]
    State(String),
    /// The engine failed in a way the caller cannot fix. The message is
    /// pre-rendered text — it MAY embed engine output verbatim for
    /// diagnostics, but the variant SHAPE stays engine-free.
    #[error("engine: {0}")]
    Engine(String),
    /// Process-spawn / filesystem failure beneath the engine.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// The git execution seam. Object-safe by contract (no generic methods —
/// consumed as `Box<dyn GitBackend>` by the executor); `async_trait` keeps
/// the returned futures `Send`.
#[async_trait]
pub trait GitBackend: Send + Sync {
    /// Create a linked worktree at `path` on a NEW branch `branch` starting
    /// at `start_point`. Returns the freshly-observed state of the new
    /// checkout. An existing branch of that name is a `State` error.
    async fn create_worktree(
        &self,
        path: &Path,
        branch: &str,
        start_point: &Sha,
    ) -> Result<WorktreeState, GitError>;

    /// Create a linked worktree at `path` with the EXISTING branch `branch`
    /// checked out, returning the new worktree's HEAD (= the branch tip).
    /// The branch must exist (`NotFound` if not); a branch already checked
    /// out in another worktree is a `State` error (git refuses to check a
    /// branch out twice); an occupied `path` is likewise a `State` error.
    ///
    /// The inter-wave widening of the frozen surface (see the module doc):
    /// added so the executor can lazily attach the integration worktree to
    /// the existing merge target (ADR-0006 User Decision 1) —
    /// [`GitBackend::create_worktree`] stays new-branch-only.
    async fn attach_worktree(&self, branch: &str, path: &Path) -> Result<Sha, GitError>;

    /// Remove the worktree at `path` (and prune its metadata). `force`
    /// discards uncommitted changes; without it a dirty worktree is a
    /// `State` error.
    async fn remove_worktree(&self, path: &Path, force: bool) -> Result<(), GitError>;

    /// Stage ALL changes (including untracked files) in `worktree` and commit
    /// them with `message`. `Ok(None)` = nothing to commit (already clean) —
    /// a normal idempotent-re-run outcome, not an error.
    async fn commit_all(&self, worktree: &Path, message: &str) -> Result<Option<Sha>, GitError>;

    /// Merge committish `source` into branch `target`, running inside
    /// `worktree` (the integration worktree). Implementations MUST verify
    /// `worktree` has `target` checked out — a mismatch is a `State` error —
    /// so a merge can never land on the wrong branch. `no_ff` forces a merge
    /// commit even when a fast-forward is possible. A conflict comes back as
    /// [`MergeResult::Conflict`], never as a `GitError`.
    async fn merge(
        &self,
        worktree: &Path,
        source: &str,
        target: &str,
        no_ff: bool,
    ) -> Result<MergeResult, GitError>;

    /// Abort the in-progress merge in `worktree`, restoring the pre-merge
    /// HEAD. No merge in progress is a `State` error.
    async fn abort_merge(&self, worktree: &Path) -> Result<(), GitError>;

    /// Apply one semantic resolution step to the in-progress merge in
    /// `worktree`. No merge in progress is a `State` error.
    async fn resolve(&self, worktree: &Path, op: ResolveOp) -> Result<ResolveOutcome, GitError>;

    /// Is `ancestor` reachable from `descendant`? This is the reachability
    /// gate the executor runs before reporting a merge outcome; a `false`
    /// here triggers the [`GitBackend::reset_hard`] rollback.
    async fn is_ancestor(&self, ancestor: &Sha, descendant: &Sha) -> Result<bool, GitError>;

    /// Does `sha` name a commit present in this repository's object DB?
    async fn commit_exists(&self, sha: &Sha) -> Result<bool, GitError>;

    /// Observe every checkout of this repository (the main worktree plus all
    /// linked ones) — the Reconcile input.
    async fn worktree_states(&self) -> Result<Vec<WorktreeState>, GitError>;

    /// The commit `HEAD` points at in `worktree`.
    async fn head_of(&self, worktree: &Path) -> Result<Sha, GitError>;

    /// Hard-reset `worktree` to `to`, discarding local changes — the
    /// reachability-gate rollback primitive.
    async fn reset_hard(&self, worktree: &Path, to: &Sha) -> Result<(), GitError>;
}

// Compile-time proof the trait stays object-safe (the executor's consumption
// shape). A generic method added to the trait breaks this line, not a distant
// downstream build.
#[allow(dead_code)]
fn _assert_object_safe(_: &dyn GitBackend) {}
