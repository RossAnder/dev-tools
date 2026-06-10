//! [`ShellGit`] — the shell-git v1 implementation of [`GitBackend`].
//!
//! Every operation is a short-lived `git` child spawned via
//! [`tokio::process::Command`]. One [`base_command`](ShellGit::base_command)
//! helper applies the scripting hygiene uniformly: env pins
//! (`GIT_TERMINAL_PROMPT=0`, `GIT_OPTIONAL_LOCKS=0`, `GIT_CONFIG_NOSYSTEM=1`,
//! `LC_ALL=C`) plus `-c core.quotepath=false` (and, on Windows,
//! `-c core.longpaths=true` — best effort: an old git ignores unknown config
//! silently). The `LC_ALL=C` pin is load-bearing: failure CLASSIFICATION
//! below string-matches git's English messages (stable under the C locale) to
//! map engine failures onto the neutral [`GitError`] taxonomy.
//!
//! Porcelain parsing stays INSIDE this module — nothing engine-shaped escapes
//! through the trait. The one deliberate exception is [`ShellGit::run_git`],
//! an INHERENT (non-trait) escape hatch quarantined off the migratable
//! surface; see its docs before reaching for it.

use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

use async_trait::async_trait;
use tokio::process::Command;

use super::{
    GitBackend, GitError, MergeResult, ResolveOp, ResolveOutcome, Sha, WorktreeState,
    WorktreeStatus,
};

/// The stable identity stamped on every commit this backend creates
/// (checkpoint commits, merge commits, resolution commits). Set via
/// `GIT_AUTHOR_*`/`GIT_COMMITTER_*` env on the child so it never depends on
/// the target repository's `user.*` config.
const IDENTITY_NAME: &str = "lumina-companion";
const IDENTITY_EMAIL: &str = "companion@lumina.local";

/// Raw result of a [`ShellGit::run_git`] invocation. Deliberately
/// shell-shaped (exit code + decoded byte streams) — this type exists ONLY on
/// the quarantined escape hatch, never on the [`GitBackend`] surface.
#[derive(Debug, Clone)]
pub struct RawGitOutput {
    /// The child's exit code; `None` if it was terminated by a signal.
    pub status: Option<i32>,
    /// stdout, lossily decoded as UTF-8, verbatim (untrimmed).
    pub stdout: String,
    /// stderr, lossily decoded as UTF-8, verbatim (untrimmed).
    pub stderr: String,
}

/// Stamp the companion's commit identity onto a git child.
fn with_identity(cmd: &mut Command) {
    cmd.env("GIT_AUTHOR_NAME", IDENTITY_NAME)
        .env("GIT_AUTHOR_EMAIL", IDENTITY_EMAIL)
        .env("GIT_COMMITTER_NAME", IDENTITY_NAME)
        .env("GIT_COMMITTER_EMAIL", IDENTITY_EMAIL);
}

/// Render a failed git invocation as [`GitError::Engine`] — pre-rendered
/// text only (the taxonomy allows embedding engine output verbatim for
/// diagnostics, but no engine TYPE may leak).
fn engine(context: &str, out: &Output) -> GitError {
    let code = out
        .status
        .code()
        .map_or_else(|| "<signal>".to_owned(), |c| c.to_string());
    let stderr = String::from_utf8_lossy(&out.stderr);
    GitError::Engine(format!("{context}: git exited {code}: {}", stderr.trim()))
}

/// Run a prepared command, mapping success to trimmed stdout and any
/// non-zero exit to [`GitError::Engine`].
async fn run_ok(mut cmd: Command, context: &str) -> Result<String, GitError> {
    let out = cmd.output().await?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        Err(engine(context, &out))
    }
}

/// Shell-git [`GitBackend`]: fronts ONE repository rooted at `repo_root`
/// (all linked worktrees share its object DB, per the trait's addressing
/// model). Construct one per repository.
pub struct ShellGit {
    repo_root: PathBuf,
}

impl ShellGit {
    /// A backend for the repository whose main worktree is at `repo_root`.
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        ShellGit {
            repo_root: repo_root.into(),
        }
    }

    /// The repository root this backend fronts.
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// The single hygiene chokepoint: every git child goes through here.
    /// `worktree`-scoped operations pass `Some(dir)` (becomes `-C <dir>`);
    /// object-DB-scoped ones pass `None` and run against the repo root.
    fn base_command(&self, worktree: Option<&Path>) -> Command {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(worktree.unwrap_or(&self.repo_root));
        cmd.args(["-c", "core.quotepath=false"]);
        if cfg!(windows) {
            // Grade-C best effort: tolerated to silently not help on exotic
            // setups; never relied on for correctness.
            cmd.args(["-c", "core.longpaths=true"]);
        }
        cmd.env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .kill_on_drop(true);
        cmd
    }

    /// Run an arbitrary `git` invocation against the repo root and return its
    /// raw output — the audited LAST-RESORT escape hatch.
    ///
    /// # Quarantine
    ///
    /// Deliberately an INHERENT method on `ShellGit`, NOT on [`GitBackend`]:
    /// it exposes the shell engine directly (argv in, exit code + byte
    /// streams out), so it cannot survive an engine swap. Every call site is
    /// a migration liability and an audit point — reach for it only when no
    /// trait method can express the operation, and prefer widening the trait
    /// (a coordinated freeze-window change) over letting call sites
    /// accumulate.
    ///
    /// Unlike the trait methods, a non-zero git exit is NOT mapped into
    /// [`GitError`]: status/stdout/stderr come back verbatim and the CALLER
    /// owns classification. `Err` here means git could not be spawned. Args
    /// may include their own `-C <dir>` to retarget a linked worktree (a
    /// later absolute `-C` overrides the root one this method injects).
    pub async fn run_git(&self, args: &[&str]) -> Result<RawGitOutput, GitError> {
        let mut cmd = self.base_command(None);
        cmd.args(args);
        let out = cmd.output().await?;
        Ok(RawGitOutput {
            status: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }

    // --- private porcelain helpers (the parsing stays in this module) ---

    /// `rev-parse HEAD` in `worktree`.
    async fn rev_parse_head(&self, worktree: &Path) -> Result<Sha, GitError> {
        let mut cmd = self.base_command(Some(worktree));
        cmd.args(["rev-parse", "HEAD"]);
        Ok(Sha::new(run_ok(cmd, "rev-parse HEAD").await?))
    }

    /// The branch checked out in `worktree`; `None` = detached HEAD
    /// (`symbolic-ref --quiet` exits 1 without output on a non-symbolic HEAD).
    async fn checked_out_branch(&self, worktree: &Path) -> Result<Option<String>, GitError> {
        let mut cmd = self.base_command(Some(worktree));
        cmd.args(["symbolic-ref", "--quiet", "--short", "HEAD"]);
        let out = cmd.output().await?;
        match out.status.code() {
            Some(0) => Ok(Some(
                String::from_utf8_lossy(&out.stdout).trim().to_owned(),
            )),
            Some(1) => Ok(None),
            _ => Err(engine("symbolic-ref HEAD", &out)),
        }
    }

    /// Is a merge in progress in `worktree`? (`MERGE_HEAD` resolvable.)
    async fn merge_in_progress(&self, worktree: &Path) -> Result<bool, GitError> {
        let mut cmd = self.base_command(Some(worktree));
        cmd.args(["rev-parse", "-q", "--verify", "MERGE_HEAD"]);
        let out = cmd.output().await?;
        match out.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(engine("rev-parse MERGE_HEAD", &out)),
        }
    }

    /// The currently-conflicted paths (repo-relative) in `worktree`.
    async fn conflicted_paths(&self, worktree: &Path) -> Result<Vec<String>, GitError> {
        let mut cmd = self.base_command(Some(worktree));
        cmd.args(["diff", "--name-only", "--diff-filter=U"]);
        let stdout = run_ok(cmd, "diff --diff-filter=U").await?;
        Ok(stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect())
    }

    /// Resolve `refname` to the commit it names in `dir`'s context, or `None`
    /// if it does not name a commit. `rev-parse -q --verify <ref>^{commit}`
    /// exits 1 quietly for both unknown refs and missing full-hex ids (the
    /// `^{commit}` peel forces an object read, so a full sha cannot
    /// false-positive).
    async fn resolve_commit(&self, dir: &Path, refname: &str) -> Result<Option<Sha>, GitError> {
        let spec = format!("{refname}^{{commit}}");
        let mut cmd = self.base_command(Some(dir));
        cmd.args(["rev-parse", "-q", "--verify", &spec]);
        let out = cmd.output().await?;
        if out.status.success() {
            Ok(Some(Sha::new(
                String::from_utf8_lossy(&out.stdout).trim().to_owned(),
            )))
        } else {
            Ok(None)
        }
    }

    /// Does `sha` name a commit in the object DB? (`cat-file -e <sha>^{commit}`,
    /// the existence pre-check; any non-signal failure means "no".)
    async fn commit_present(&self, sha: &Sha) -> Result<bool, GitError> {
        let spec = format!("{sha}^{{commit}}");
        let mut cmd = self.base_command(None);
        cmd.args(["cat-file", "-e", &spec]);
        let out = cmd.output().await?;
        if out.status.success() {
            Ok(true)
        } else if out.status.code().is_some() {
            Ok(false)
        } else {
            Err(engine("cat-file -e", &out))
        }
    }

    /// Clean/dirty/conflicted classification of `worktree`: clean ⟺ zero
    /// non-`#` lines from `status --porcelain=v2 --branch`; conflicted ⟺
    /// `MERGE_HEAD` present AND unresolved conflict entries remain.
    async fn status_of(&self, worktree: &Path) -> Result<WorktreeStatus, GitError> {
        let mut cmd = self.base_command(Some(worktree));
        cmd.args(["status", "--porcelain=v2", "--branch"]);
        let stdout = run_ok(cmd, "status --porcelain=v2").await?;
        let dirty = stdout
            .lines()
            .any(|l| !l.is_empty() && !l.starts_with('#'));
        if !dirty {
            return Ok(WorktreeStatus::Clean);
        }
        if self.merge_in_progress(worktree).await?
            && !self.conflicted_paths(worktree).await?.is_empty()
        {
            Ok(WorktreeStatus::Conflicted)
        } else {
            Ok(WorktreeStatus::Dirty)
        }
    }

    /// Freshly observe one checkout (branch + head + status) at `path`.
    async fn observe(&self, path: &Path) -> Result<WorktreeState, GitError> {
        Ok(WorktreeState {
            path: path.to_path_buf(),
            branch: self.checked_out_branch(path).await?,
            head: self.rev_parse_head(path).await?,
            status: self.status_of(path).await?,
        })
    }

    /// The named paths, or — when the list is empty (the "every
    /// currently-conflicted path" convention) — the live conflict set.
    async fn effective_conflict_paths(
        &self,
        worktree: &Path,
        paths: Vec<String>,
    ) -> Result<Vec<String>, GitError> {
        if paths.is_empty() {
            self.conflicted_paths(worktree).await
        } else {
            Ok(paths)
        }
    }

    /// `git add -- <paths>` (mark resolved as-staged).
    async fn stage_resolved_paths(&self, worktree: &Path, paths: &[String]) -> Result<(), GitError> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut cmd = self.base_command(Some(worktree));
        cmd.args(["add", "--"]).args(paths);
        let out = cmd.output().await?;
        if out.status.success() {
            Ok(())
        } else {
            Err(engine("resolve: stage", &out))
        }
    }

    /// Shared body of `TakeOurs`/`TakeTheirs`: check out one side of the
    /// conflicted paths, stage them, report what remains.
    async fn take_side(
        &self,
        worktree: &Path,
        side: &str,
        paths: Vec<String>,
    ) -> Result<ResolveOutcome, GitError> {
        let paths = self.effective_conflict_paths(worktree, paths).await?;
        if !paths.is_empty() {
            let mut cmd = self.base_command(Some(worktree));
            cmd.args(["checkout", side, "--"]).args(&paths);
            let out = cmd.output().await?;
            if !out.status.success() {
                return Err(engine("resolve: checkout side", &out));
            }
            self.stage_resolved_paths(worktree, &paths).await?;
        }
        Ok(ResolveOutcome::Pending {
            remaining: self.conflicted_paths(worktree).await?,
        })
    }

    /// `git merge --abort` in `worktree` (caller has verified a merge is in
    /// progress).
    async fn run_merge_abort(&self, worktree: &Path) -> Result<(), GitError> {
        let mut cmd = self.base_command(Some(worktree));
        cmd.args(["merge", "--abort"]);
        let out = cmd.output().await?;
        if out.status.success() {
            Ok(())
        } else {
            Err(engine("merge --abort", &out))
        }
    }
}

#[async_trait]
impl GitBackend for ShellGit {
    async fn create_worktree(
        &self,
        path: &Path,
        branch: &str,
        start_point: &Sha,
    ) -> Result<WorktreeState, GitError> {
        let mut cmd = self.base_command(None);
        cmd.args(["worktree", "add"])
            .arg(path)
            .args(["-b", branch])
            .arg(start_point.as_str());
        let out = cmd.output().await?;
        if !out.status.success() {
            // `worktree add` is atomic (it refuses rather than half-creates),
            // so classification is all that's left. C-locale messages:
            //   "a branch named '…' already exists"          -> State
            //   "'…' already exists" (occupied path)         -> State
            //   "'…' is already used by worktree at …"       -> State
            //   "invalid reference: …" / "not a valid …"     -> NotFound
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            let lower = stderr.to_lowercase();
            return Err(
                if lower.contains("already exists") || lower.contains("already used by worktree")
                {
                    GitError::State(format!("create_worktree: {stderr}"))
                } else if lower.contains("invalid reference")
                    || lower.contains("not a valid object name")
                {
                    GitError::NotFound(format!("create_worktree: {stderr}"))
                } else {
                    engine("create_worktree", &out)
                },
            );
        }
        self.observe(path).await
    }

    async fn attach_worktree(&self, branch: &str, path: &Path) -> Result<Sha, GitError> {
        // No `-b`: checks out the EXISTING branch (the inter-wave widening;
        // see the trait doc). `worktree add` stays atomic here too.
        let mut cmd = self.base_command(None);
        cmd.args(["worktree", "add"]).arg(path).arg(branch);
        let out = cmd.output().await?;
        if !out.status.success() {
            // C-locale messages:
            //   "'…' already exists" (occupied path)         -> State
            //   "'…' is already used by worktree at …"
            //     (branch checked out elsewhere)              -> State
            //   "invalid reference: …" (no such branch — git
            //     resolves the bare argument as a committish)  -> NotFound
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            let lower = stderr.to_lowercase();
            return Err(
                if lower.contains("already exists") || lower.contains("already used by worktree")
                {
                    GitError::State(format!("attach_worktree: {stderr}"))
                } else if lower.contains("invalid reference")
                    || lower.contains("not a valid object name")
                {
                    GitError::NotFound(format!("attach_worktree: {stderr}"))
                } else {
                    engine("attach_worktree", &out)
                },
            );
        }
        self.rev_parse_head(path).await
    }

    async fn remove_worktree(&self, path: &Path, force: bool) -> Result<(), GitError> {
        let mut cmd = self.base_command(None);
        cmd.args(["worktree", "remove"]);
        if force {
            cmd.arg("--force");
        }
        cmd.arg(path);
        let out = cmd.output().await?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        let lower = stderr.to_lowercase();
        Err(
            if lower.contains("contains modified or untracked files")
                || lower.contains("is a main working tree")
            {
                GitError::State(format!("remove_worktree: {stderr}"))
            } else if lower.contains("is not a working tree") {
                GitError::NotFound(format!("remove_worktree: {stderr}"))
            } else {
                engine("remove_worktree", &out)
            },
        )
    }

    async fn commit_all(&self, worktree: &Path, message: &str) -> Result<Option<Sha>, GitError> {
        // Anything to commit? (`status --porcelain` empty ⟺ clean.)
        let mut cmd = self.base_command(Some(worktree));
        cmd.args(["status", "--porcelain"]);
        if run_ok(cmd, "commit_all: status").await?.is_empty() {
            return Ok(None);
        }
        let mut cmd = self.base_command(Some(worktree));
        cmd.args(["add", "-A"]);
        run_ok(cmd, "commit_all: add").await?;
        let mut cmd = self.base_command(Some(worktree));
        with_identity(&mut cmd);
        cmd.args(["commit", "-m", message]);
        run_ok(cmd, "commit_all: commit").await?;
        Ok(Some(self.rev_parse_head(worktree).await?))
    }

    async fn merge(
        &self,
        worktree: &Path,
        source: &str,
        target: &str,
        no_ff: bool,
    ) -> Result<MergeResult, GitError> {
        // Contract: the merge can never land on the wrong branch.
        match self.checked_out_branch(worktree).await? {
            Some(b) if b == target => {}
            Some(b) => {
                return Err(GitError::State(format!(
                    "merge: worktree {} has '{b}' checked out, not target '{target}'",
                    worktree.display()
                )));
            }
            None => {
                return Err(GitError::State(format!(
                    "merge: worktree {} is on a detached HEAD, not target '{target}'",
                    worktree.display()
                )));
            }
        }
        // Resolve the source tip up front: it is both the NotFound pre-check
        // and the fast-forward discriminator below.
        let Some(src) = self.resolve_commit(worktree, source).await? else {
            return Err(GitError::NotFound(format!(
                "merge: source '{source}' does not name a commit"
            )));
        };
        let pre = self.rev_parse_head(worktree).await?;

        let mut cmd = self.base_command(Some(worktree));
        with_identity(&mut cmd);
        cmd.args(["merge", "--no-edit"]);
        if no_ff {
            cmd.arg("--no-ff");
        }
        cmd.arg(source);
        let out = cmd.output().await?;

        if out.status.success() {
            // Classify by HEAD movement: unmoved = already up to date; moved
            // exactly to the source tip = fast-forward (no commit created);
            // moved to a NEW commit = a merge commit. (More robust than a
            // HEAD^2 probe — a fast-forward ONTO a merge commit has a second
            // parent too.)
            let post = self.rev_parse_head(worktree).await?;
            return Ok(if post == pre {
                MergeResult::AlreadyUpToDate
            } else if post == src {
                MergeResult::FastForward { new_tip: post }
            } else {
                MergeResult::Merged { merge_sha: post }
            });
        }
        // Non-zero exit: a conflict is a NORMAL outcome — classify it as
        // MERGE_HEAD present AND unresolved entries non-empty; anything else
        // non-zero is an engine failure.
        if self.merge_in_progress(worktree).await? {
            let paths = self.conflicted_paths(worktree).await?;
            if !paths.is_empty() {
                return Ok(MergeResult::Conflict { paths });
            }
        }
        Err(engine("merge", &out))
    }

    async fn abort_merge(&self, worktree: &Path) -> Result<(), GitError> {
        if !self.merge_in_progress(worktree).await? {
            return Err(GitError::State(format!(
                "abort_merge: no merge in progress in {}",
                worktree.display()
            )));
        }
        self.run_merge_abort(worktree).await
    }

    async fn resolve(&self, worktree: &Path, op: ResolveOp) -> Result<ResolveOutcome, GitError> {
        if !self.merge_in_progress(worktree).await? {
            return Err(GitError::State(format!(
                "resolve: no merge in progress in {}",
                worktree.display()
            )));
        }
        match op {
            ResolveOp::TakeOurs { paths } => self.take_side(worktree, "--ours", paths).await,
            ResolveOp::TakeTheirs { paths } => self.take_side(worktree, "--theirs", paths).await,
            ResolveOp::StageResolution { paths } => {
                let paths = self.effective_conflict_paths(worktree, paths).await?;
                self.stage_resolved_paths(worktree, &paths).await?;
                Ok(ResolveOutcome::Pending {
                    remaining: self.conflicted_paths(worktree).await?,
                })
            }
            ResolveOp::Continue => {
                let remaining = self.conflicted_paths(worktree).await?;
                if !remaining.is_empty() {
                    // Not completable yet — report what's left rather than
                    // letting the commit fail with engine noise.
                    return Ok(ResolveOutcome::Pending { remaining });
                }
                let mut cmd = self.base_command(Some(worktree));
                with_identity(&mut cmd);
                cmd.args(["commit", "--no-edit"]);
                let out = cmd.output().await?;
                if !out.status.success() {
                    return Err(engine("resolve: continue", &out));
                }
                Ok(ResolveOutcome::Completed {
                    merge_sha: self.rev_parse_head(worktree).await?,
                })
            }
            ResolveOp::Abort => {
                self.run_merge_abort(worktree).await?;
                Ok(ResolveOutcome::Aborted)
            }
        }
    }

    async fn is_ancestor(&self, ancestor: &Sha, descendant: &Sha) -> Result<bool, GitError> {
        // Existence pre-check: `merge-base --is-ancestor` cannot distinguish
        // "not an ancestor" from "no such commit" by message alone.
        for sha in [ancestor, descendant] {
            if !self.commit_present(sha).await? {
                return Err(GitError::NotFound(format!(
                    "is_ancestor: '{sha}' does not name a commit"
                )));
            }
        }
        let mut cmd = self.base_command(None);
        cmd.args(["merge-base", "--is-ancestor", ancestor.as_str(), descendant.as_str()]);
        let out = cmd.output().await?;
        match out.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(engine("is_ancestor", &out)),
        }
    }

    async fn commit_exists(&self, sha: &Sha) -> Result<bool, GitError> {
        self.commit_present(sha).await
    }

    async fn worktree_states(&self) -> Result<Vec<WorktreeState>, GitError> {
        let mut cmd = self.base_command(None);
        cmd.args(["worktree", "list", "--porcelain", "-z"]);
        let out = cmd.output().await?;
        if !out.status.success() {
            return Err(engine("worktree list", &out));
        }
        // -z framing: each attribute NUL-terminated, records separated by an
        // empty (NUL-only) line — i.e. `\0\0` between records. Paths cannot
        // contain NUL, so the split is unambiguous.
        let text = String::from_utf8_lossy(&out.stdout);
        let mut checkouts = Vec::new();
        for record in text.split("\0\0") {
            let mut path: Option<PathBuf> = None;
            let mut head: Option<String> = None;
            let mut branch: Option<String> = None;
            let mut bare = false;
            for field in record.split('\0').filter(|f| !f.is_empty()) {
                if let Some(p) = field.strip_prefix("worktree ") {
                    path = Some(PathBuf::from(p));
                } else if let Some(h) = field.strip_prefix("HEAD ") {
                    head = Some(h.to_owned());
                } else if let Some(b) = field.strip_prefix("branch ") {
                    branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_owned());
                } else if field == "bare" {
                    bare = true;
                }
                // "detached" needs no handling (branch stays None);
                // "locked …" / "prunable …" are ignored.
            }
            let Some(path) = path else { continue };
            if bare {
                continue; // a bare entry has no checkout to classify
            }
            let head = head.ok_or_else(|| {
                GitError::Engine(format!(
                    "worktree list: record for {} carries no HEAD",
                    path.display()
                ))
            })?;
            checkouts.push((path, head, branch));
        }
        let mut states = Vec::with_capacity(checkouts.len());
        for (path, head, branch) in checkouts {
            let status = self.status_of(&path).await?;
            states.push(WorktreeState {
                path,
                branch,
                head: Sha::new(head),
                status,
            });
        }
        Ok(states)
    }

    async fn head_of(&self, worktree: &Path) -> Result<Sha, GitError> {
        self.rev_parse_head(worktree).await
    }

    async fn reset_hard(&self, worktree: &Path, to: &Sha) -> Result<(), GitError> {
        if !self.commit_present(to).await? {
            return Err(GitError::NotFound(format!(
                "reset_hard: '{to}' does not name a commit"
            )));
        }
        let mut cmd = self.base_command(Some(worktree));
        cmd.args(["reset", "--hard", to.as_str()]);
        let out = cmd.output().await?;
        if out.status.success() {
            Ok(())
        } else {
            Err(engine("reset_hard", &out))
        }
    }
}
