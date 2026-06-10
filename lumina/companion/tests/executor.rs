//! Executor intent->outcome tests against the scripted [`FakeGitBackend`] —
//! deterministic, no git binary, no real repository. The only filesystem
//! touch is the `.git/info/exclude` registration, exercised against a
//! fabricated `<tempdir>/.git/info/` per test.
//!
//! The merge tests pin the DETACHED-integration ref-CAS choreography:
//! resolve target tip -> observe worktrees -> ensure a detached integration
//! checkout at that tip (attach-detached when missing; abort-then-detach when
//! present) -> HEAD sanity guard -> merge -> reachability gate -> atomic
//! `update_branch_ref` compare-and-swap -> `target_checkout` operator hint.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lumina_companion::executor::{
    Executor, WORKTREES_EXCLUDE_ENTRY, ensure_worktrees_excluded,
};
use lumina_companion::git::{
    GitError, MergeResult, Sha, WorktreeState, WorktreeStatus,
    fake::{FakeCall, FakeGitBackend},
};
use lumina_protocol::{
    FailureKind, Intent, Outcome, Sha as WireSha, TargetCheckoutHint, WorktreeSnapshot,
};
use tempfile::TempDir;

/// A fresh fabricated repo root under a process-private [`TempDir`]:
/// `<temp>/.git/info/` exists, nothing else. The `TempDir` guard is RETURNED
/// alongside the path and MUST be held for the test's lifetime — dropping it
/// deletes the directory out from under the executor.
fn temp_repo_root(_tag: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join(".git").join("info")).unwrap();
    (dir, root)
}

/// A fabricated repo root whose `.git` is a FILE, not a directory — the
/// linked-worktree shape the module doc promises "fails cleanly as
/// `FailureKind::Internal`": `create_dir_all(.git/info)` cannot descend
/// through a `.git` file, so the exclude registration fails.
fn temp_repo_root_with_git_file(_tag: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::write(root.join(".git"), b"gitdir: /elsewhere/.git/worktrees/wt\n").unwrap();
    (dir, root)
}

/// Executor over a fake backend, keeping the concrete handle for call-log
/// assertions (the `Arc<dyn GitBackend>` consumption shape). The returned
/// [`TempDir`] guard must outlive the executor.
fn executor(tag: &str) -> (Executor, Arc<FakeGitBackend>, PathBuf, TempDir) {
    let (dir, root) = temp_repo_root(tag);
    let fake = Arc::new(FakeGitBackend::new());
    let exec = Executor::new(root.clone(), fake.clone());
    (exec, fake, root, dir)
}

/// Like [`executor`], but the repo root's `.git` is a FILE (linked-worktree
/// shape) — the exclude registration fails cleanly as `FailureKind::Internal`.
fn executor_with_git_file(tag: &str) -> (Executor, Arc<FakeGitBackend>, PathBuf, TempDir) {
    let (dir, root) = temp_repo_root_with_git_file(tag);
    let fake = Arc::new(FakeGitBackend::new());
    let exec = Executor::new(root.clone(), fake.clone());
    (exec, fake, root, dir)
}

fn wt(path: &Path, branch: Option<&str>, head: &str, status: WorktreeStatus) -> WorktreeState {
    WorktreeState {
        path: path.to_path_buf(),
        branch: branch.map(str::to_owned),
        head: Sha::new(head),
        status,
    }
}

fn merge_intent(reachable: &[&str]) -> Intent {
    Intent::MergeWorktree {
        source_branch: "sprint/serene-1".to_owned(),
        target_branch: "main".to_owned(),
        must_remain_reachable: reachable.iter().map(|s| WireSha((*s).to_owned())).collect(),
        no_ff: true,
    }
}

/// The reflog message the choreography stamps on the CAS for
/// [`merge_intent`]'s source branch.
const REFLOG_MSG: &str = "lumina-companion: merge sprint/serene-1";

fn exclude_contents(root: &Path) -> String {
    std::fs::read_to_string(root.join(".git").join("info").join("exclude")).unwrap()
}

// --- the three simple intents -------------------------------------------

#[tokio::test]
async fn create_worktree_resolves_committish_base_and_registers_exclude() {
    let (exec, fake, root, _guard) = executor("create");
    let expected_path = exec.worktree_path_for_branch("sprint/serene-1");
    // `base` rides the wire as a COMMITTISH string ("main") and the companion
    // resolves it to a commit before any worktree is touched.
    fake.push_resolve_committish(Ok(Sha::new("base-1")));
    fake.push_create_worktree(Ok(wt(
        &expected_path,
        Some("sprint/serene-1"),
        "head-1",
        WorktreeStatus::Clean,
    )));

    let outcome = exec
        .execute(Intent::CreateWorktree {
            branch: "sprint/serene-1".to_owned(),
            base: "main".to_owned(),
        })
        .await;

    assert_eq!(
        outcome,
        Outcome::WorktreeCreated {
            path: expected_path.display().to_string(),
            branch: "sprint/serene-1".to_owned(),
            head: WireSha("head-1".to_owned()),
        }
    );
    // The sanitised path lives under the managed root and the slash became -.
    assert!(expected_path.starts_with(root.join(".lumina").join("worktrees")));
    assert!(expected_path.ends_with("sprint-serene-1"));
    assert_eq!(
        fake.calls(),
        vec![
            FakeCall::ResolveCommittish {
                committish: "main".to_owned(),
            },
            FakeCall::CreateWorktree {
                path: expected_path,
                branch: "sprint/serene-1".to_owned(),
                start_point: Sha::new("base-1"),
            },
        ]
    );
    assert!(exclude_contents(&root).contains(WORKTREES_EXCLUDE_ENTRY));
}

#[tokio::test]
async fn create_worktree_fails_not_found_on_unresolvable_base() {
    let (exec, fake, _root, _guard) = executor("create-bad-base");
    fake.push_resolve_committish(Err(GitError::NotFound(
        "resolve_committish: 'no-such-rev' does not name a commit".to_owned(),
    )));

    let outcome = exec
        .execute(Intent::CreateWorktree {
            branch: "sprint/serene-1".to_owned(),
            base: "no-such-rev".to_owned(),
        })
        .await;

    assert!(matches!(
        outcome,
        Outcome::Failed {
            kind: FailureKind::NotFound,
            ..
        }
    ));
    // Resolution failed before any worktree call.
    assert_eq!(
        fake.calls(),
        vec![FakeCall::ResolveCommittish {
            committish: "no-such-rev".to_owned(),
        }]
    );
}

#[tokio::test]
async fn remove_worktree_passes_through_and_maps_dirty_state() {
    let (exec, fake, _root, _guard) = executor("remove");
    fake.push_remove_worktree(Ok(()));
    fake.push_remove_worktree(Err(GitError::State("uncommitted changes".to_owned())));

    let removed = exec
        .execute(Intent::RemoveWorktree {
            path: "/wt/sprint-1".to_owned(),
            force: false,
        })
        .await;
    assert_eq!(removed, Outcome::WorktreeRemoved);

    let refused = exec
        .execute(Intent::RemoveWorktree {
            path: "/wt/sprint-2".to_owned(),
            force: false,
        })
        .await;
    assert!(matches!(
        refused,
        Outcome::Failed {
            kind: FailureKind::DirtyWorktree,
            ..
        }
    ));
    assert_eq!(
        fake.calls(),
        vec![
            FakeCall::RemoveWorktree {
                path: PathBuf::from("/wt/sprint-1"),
                force: false,
            },
            FakeCall::RemoveWorktree {
                path: PathBuf::from("/wt/sprint-2"),
                force: false,
            },
        ]
    );
}

#[tokio::test]
async fn commit_checkpoint_reports_commit_or_already_up_to_date() {
    let (exec, fake, _root, _guard) = executor("checkpoint");
    fake.push_commit_all(Ok(Some(Sha::new("commit-1"))));
    fake.push_commit_all(Ok(None));
    fake.push_head_of(Ok(Sha::new("tip-1")));

    let committed = exec
        .execute(Intent::CommitCheckpoint {
            path: "/wt/sprint-1".to_owned(),
            message: "checkpoint: batch 2".to_owned(),
        })
        .await;
    assert_eq!(
        committed,
        Outcome::Checkpointed {
            commit_sha: WireSha("commit-1".to_owned()),
        }
    );

    // Nothing to commit -> AlreadyUpToDate with the worktree's current HEAD.
    let noop = exec
        .execute(Intent::CommitCheckpoint {
            path: "/wt/sprint-1".to_owned(),
            message: "checkpoint: rerun".to_owned(),
        })
        .await;
    assert_eq!(
        noop,
        Outcome::AlreadyUpToDate {
            tip: WireSha("tip-1".to_owned()),
        }
    );
}

// --- merge choreography ---------------------------------------------------

#[tokio::test]
async fn merge_happy_detached_path_attaches_merges_and_advances_the_ref() {
    let (exec, fake, root, _guard) = executor("merge-happy");
    let integ = exec.integration_worktree_path("main");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    // No integration worktree yet; the primary checkout is on another branch
    // (so no target_checkout hint either).
    fake.push_worktree_states(Ok(vec![wt(
        &root,
        Some("dev"),
        "dev-tip",
        WorktreeStatus::Clean,
    )]));
    fake.push_attach_worktree_detached(Ok(Sha::new("tip-0")));
    fake.push_head_of(Ok(Sha::new("tip-0")));
    fake.push_merge(Ok(MergeResult::Merged {
        merge_sha: Sha::new("merge-1"),
    }));
    fake.push_is_ancestor(Ok(true));
    fake.push_is_ancestor(Ok(true));
    fake.push_update_branch_ref(Ok(()));

    let outcome = exec.execute(merge_intent(&["keep-1", "keep-2"])).await;

    assert_eq!(
        outcome,
        Outcome::Merged {
            merge_sha: WireSha("merge-1".to_owned()),
            fast_forward: false,
            target_checkout: None,
        }
    );
    // The FULL choreography, in order, CAS included.
    assert_eq!(
        fake.calls(),
        vec![
            FakeCall::ResolveBranchTip {
                branch: "main".to_owned(),
            },
            FakeCall::WorktreeStates,
            FakeCall::AttachWorktreeDetached {
                path: integ.clone(),
                committish: "tip-0".to_owned(),
            },
            FakeCall::HeadOf {
                worktree: integ.clone(),
            },
            FakeCall::Merge {
                worktree: integ,
                source: "sprint/serene-1".to_owned(),
                no_ff: true,
            },
            FakeCall::IsAncestor {
                ancestor: Sha::new("keep-1"),
                descendant: Sha::new("merge-1"),
            },
            FakeCall::IsAncestor {
                ancestor: Sha::new("keep-2"),
                descendant: Sha::new("merge-1"),
            },
            FakeCall::UpdateBranchRef {
                branch: "main".to_owned(),
                new: Sha::new("merge-1"),
                expected_old: Sha::new("tip-0"),
                reflog_msg: REFLOG_MSG.to_owned(),
            },
        ]
    );
}

#[tokio::test]
async fn merge_fast_forward_repins_existing_worktree_and_reports_fast_forward() {
    let (exec, fake, root, _guard) = executor("merge-ff");
    let integ = exec.integration_worktree_path("main");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    // The integration worktree already exists, DETACHED at an older tip — it
    // is re-pinned to the freshly-resolved tip before merging.
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, None, "tip-old", WorktreeStatus::Clean),
    ]));
    fake.push_detach_worktree(Ok(Sha::new("tip-0")));
    fake.push_head_of(Ok(Sha::new("tip-0")));
    fake.push_merge(Ok(MergeResult::FastForward {
        new_tip: Sha::new("ff-1"),
    }));
    fake.push_is_ancestor(Ok(true));
    fake.push_update_branch_ref(Ok(()));

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    assert_eq!(
        outcome,
        Outcome::Merged {
            merge_sha: WireSha("ff-1".to_owned()),
            fast_forward: true,
            target_checkout: None,
        }
    );
    let calls = fake.calls();
    assert!(calls.contains(&FakeCall::DetachWorktree {
        worktree: integ,
        committish: "tip-0".to_owned(),
    }));
    assert_eq!(
        calls.last(),
        Some(&FakeCall::UpdateBranchRef {
            branch: "main".to_owned(),
            new: Sha::new("ff-1"),
            expected_old: Sha::new("tip-0"),
            reflog_msg: REFLOG_MSG.to_owned(),
        })
    );
}

#[tokio::test]
async fn merge_already_up_to_date_skips_the_cas_entirely() {
    let (exec, fake, root, _guard) = executor("merge-noop");
    let integ = exec.integration_worktree_path("main");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, None, "tip-0", WorktreeStatus::Clean),
    ]));
    fake.push_detach_worktree(Ok(Sha::new("tip-0")));
    fake.push_head_of(Ok(Sha::new("tip-0")));
    fake.push_merge(Ok(MergeResult::AlreadyUpToDate));
    // NOTE: no is_ancestor and no update_branch_ref scripted — with HEAD
    // pinned at the tip, "unmoved" ⟺ source already reachable: neither the
    // gate nor the CAS may run on a no-op.

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    assert_eq!(
        outcome,
        Outcome::AlreadyUpToDate {
            tip: WireSha("tip-0".to_owned()),
        }
    );
    let calls = fake.calls();
    assert!(
        !calls
            .iter()
            .any(|c| matches!(c, FakeCall::UpdateBranchRef { .. })),
        "AlreadyUpToDate must skip the CAS, got {calls:?}"
    );
}

#[tokio::test]
async fn merge_conflict_aborts_locally_then_reports_conflicted() {
    let (exec, fake, root, _guard) = executor("merge-conflict");
    let integ = exec.integration_worktree_path("main");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, None, "tip-0", WorktreeStatus::Clean),
    ]));
    fake.push_detach_worktree(Ok(Sha::new("tip-0")));
    fake.push_head_of(Ok(Sha::new("tip-0")));
    fake.push_merge(Ok(MergeResult::Conflict {
        paths: vec!["src/lib.rs".to_owned(), "Cargo.toml".to_owned()],
    }));
    fake.push_abort_merge(Ok(()));

    let outcome = exec.execute(merge_intent(&[])).await;
    assert_eq!(
        outcome,
        Outcome::Conflicted {
            paths: vec!["src/lib.rs".to_owned(), "Cargo.toml".to_owned()],
        }
    );
    // The coarse-protocol rule: abort_merge ran AFTER the merge, before the
    // outcome was reported — and the branch ref was never touched.
    let calls = fake.calls();
    assert_eq!(
        calls.last(),
        Some(&FakeCall::AbortMerge {
            worktree: integ.clone(),
        })
    );
    assert!(calls.contains(&FakeCall::Merge {
        worktree: integ,
        source: "sprint/serene-1".to_owned(),
        no_ff: true,
    }));
    assert!(
        !calls
            .iter()
            .any(|c| matches!(c, FakeCall::UpdateBranchRef { .. })),
        "a conflicted merge must not move the ref"
    );
}

#[tokio::test]
async fn merge_gate_violation_rolls_back_before_any_ref_move() {
    let (exec, fake, root, _guard) = executor("merge-gate");
    let integ = exec.integration_worktree_path("main");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, None, "tip-0", WorktreeStatus::Clean),
    ]));
    fake.push_detach_worktree(Ok(Sha::new("tip-0")));
    fake.push_head_of(Ok(Sha::new("tip-0")));
    fake.push_merge(Ok(MergeResult::Merged {
        merge_sha: Sha::new("merge-1"),
    }));
    fake.push_is_ancestor(Ok(true));
    fake.push_is_ancestor(Ok(false)); // keep-2 lost -> ADR §H violation
    fake.push_reset_hard(Ok(()));

    let outcome = exec.execute(merge_intent(&["keep-1", "keep-2"])).await;
    assert!(matches!(
        outcome,
        Outcome::Failed {
            kind: FailureKind::ReachabilityViolation,
            ..
        }
    ));
    // The rollback went through the backend, to the resolved pre-merge tip —
    // and the gate fired BEFORE the CAS, so the real branch never moved.
    let calls = fake.calls();
    assert_eq!(
        calls.last(),
        Some(&FakeCall::ResetHard {
            worktree: integ,
            to: Sha::new("tip-0"),
        })
    );
    assert!(
        !calls
            .iter()
            .any(|c| matches!(c, FakeCall::UpdateBranchRef { .. })),
        "a failed gate must never reach the CAS"
    );
}

#[tokio::test]
async fn merge_gate_violation_with_failed_rollback_double_faults_to_internal() {
    // The double-fault arm (executor.rs rollback_failed Err branch): the gate
    // fires (keep-2 lost) AND the rollback reset itself fails. The outcome
    // escalates to Internal and the message carries BOTH the gate-violation
    // context and the 'ADDITIONALLY the rollback' context.
    let (exec, fake, root, _guard) = executor("merge-gate-double-fault");
    let integ = exec.integration_worktree_path("main");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, None, "tip-0", WorktreeStatus::Clean),
    ]));
    fake.push_detach_worktree(Ok(Sha::new("tip-0")));
    fake.push_head_of(Ok(Sha::new("tip-0")));
    fake.push_merge(Ok(MergeResult::Merged {
        merge_sha: Sha::new("merge-1"),
    }));
    fake.push_is_ancestor(Ok(true));
    fake.push_is_ancestor(Ok(false)); // keep-2 lost -> ADR §H violation
    fake.push_reset_hard(Err(GitError::Engine(
        "reset --hard: unable to update HEAD".to_owned(),
    ))); // the rollback ALSO fails -> double fault

    let outcome = exec.execute(merge_intent(&["keep-1", "keep-2"])).await;
    let Outcome::Failed { kind, message } = outcome else {
        panic!("expected Failed, got {outcome:?}");
    };
    // A failed rollback escalates to Internal regardless of the gate's kind.
    assert_eq!(kind, FailureKind::Internal);
    // Both contexts survive into the one message: the gate violation ("left
    // ... unreachable") AND the rollback failure ("ADDITIONALLY the rollback").
    assert!(
        message.contains("unreachable from new tip"),
        "missing gate-violation context: {message}"
    );
    assert!(
        message.contains("ADDITIONALLY the rollback to pre-merge tip"),
        "missing rollback-failure context: {message}"
    );
    // The CAS was never reached.
    let calls = fake.calls();
    assert!(
        !calls
            .iter()
            .any(|c| matches!(c, FakeCall::UpdateBranchRef { .. })),
        "a failed gate must never reach the CAS, got {calls:?}"
    );
}

#[tokio::test]
async fn merge_unverifiable_gate_rolls_back_and_reports_the_check_failure() {
    // The unverifiable-gate arm (executor.rs is_ancestor Err branch): the gate
    // check ERRORS rather than answering false. An unverifiable merge is never
    // left in place — it rolls back and reports the 'reachability check'
    // failure, with the kind `kind_for` maps for the scripted GitError.
    let (exec, fake, root, _guard) = executor("merge-gate-unverifiable");
    let integ = exec.integration_worktree_path("main");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, None, "tip-0", WorktreeStatus::Clean),
    ]));
    fake.push_detach_worktree(Ok(Sha::new("tip-0")));
    fake.push_head_of(Ok(Sha::new("tip-0")));
    fake.push_merge(Ok(MergeResult::Merged {
        merge_sha: Sha::new("merge-1"),
    }));
    // The gate check itself fails to run — an Engine error maps to GitFailure.
    fake.push_is_ancestor(Err(GitError::Engine(
        "merge-base --is-ancestor: object not found".to_owned(),
    )));
    fake.push_reset_hard(Ok(()));

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    let Outcome::Failed { kind, message } = outcome else {
        panic!("expected Failed, got {outcome:?}");
    };
    // `Engine` -> `GitFailure` per the kind table.
    assert_eq!(kind, FailureKind::GitFailure);
    assert!(
        message.contains("reachability check"),
        "missing reachability-check failure message: {message}"
    );
    // The merge rolled back: reset_hard ran to the pre-merge tip, AFTER the
    // failed gate, and the real branch ref never moved.
    let calls = fake.calls();
    assert_eq!(
        calls.last(),
        Some(&FakeCall::ResetHard {
            worktree: integ,
            to: Sha::new("tip-0"),
        })
    );
    assert!(
        !calls
            .iter()
            .any(|c| matches!(c, FakeCall::UpdateBranchRef { .. })),
        "an unverifiable gate must never reach the CAS, got {calls:?}"
    );
}

#[tokio::test]
async fn merge_cas_lost_reports_target_moved_without_rollback() {
    let (exec, fake, root, _guard) = executor("merge-cas-lost");
    let integ = exec.integration_worktree_path("main");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, None, "tip-0", WorktreeStatus::Clean),
    ]));
    fake.push_detach_worktree(Ok(Sha::new("tip-0")));
    fake.push_head_of(Ok(Sha::new("tip-0")));
    fake.push_merge(Ok(MergeResult::Merged {
        merge_sha: Sha::new("merge-1"),
    }));
    fake.push_is_ancestor(Ok(true));
    // The operator committed to `main` between tip-resolve and the CAS.
    fake.push_update_branch_ref(Err(GitError::RefCasLost(
        "update_branch_ref: error: cannot lock ref 'refs/heads/main': \
         is at operator-tip but expected tip-0"
            .to_owned(),
    )));

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    assert!(matches!(
        outcome,
        Outcome::Failed {
            kind: FailureKind::TargetMoved,
            ..
        }
    ));
    // NO rollback after a lost CAS: no ref was touched and the orphan merge
    // commit stays reflog-protected; the next run re-detaches at the new tip.
    let calls = fake.calls();
    assert!(
        !calls.iter().any(|c| matches!(c, FakeCall::ResetHard { .. })),
        "a lost CAS must not trigger reset_hard, got {calls:?}"
    );
    assert!(matches!(
        calls.last(),
        Some(FakeCall::UpdateBranchRef { .. })
    ));
}

#[tokio::test]
async fn merge_refuses_dirty_integration_worktree_before_merging() {
    let (exec, fake, root, _guard) = executor("merge-dirty");
    let integ = exec.integration_worktree_path("main");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, None, "tip-old", WorktreeStatus::Dirty),
    ]));

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    assert!(matches!(
        outcome,
        Outcome::Failed {
            kind: FailureKind::DirtyWorktree,
            ..
        }
    ));
    // Refused on the ground-truth observation alone — no detach, no merge.
    assert_eq!(
        fake.calls(),
        vec![
            FakeCall::ResolveBranchTip {
                branch: "main".to_owned(),
            },
            FakeCall::WorktreeStates,
        ]
    );
}

#[tokio::test]
async fn merge_migrates_legacy_on_branch_worktree_with_abort_then_detach() {
    let (exec, fake, root, _guard) = executor("merge-legacy");
    let integ = exec.integration_worktree_path("main");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    // A LEGACY integration worktree: ON the target branch, with an
    // interrupted merge left behind by a crashed prior run.
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, Some("main"), "tip-0", WorktreeStatus::Conflicted),
    ]));
    fake.push_abort_merge(Ok(())); // the self-heal: lands back ON the branch
    fake.push_detach_worktree(Ok(Sha::new("tip-0"))); // then migrate off it
    fake.push_head_of(Ok(Sha::new("tip-0")));
    fake.push_merge(Ok(MergeResult::Merged {
        merge_sha: Sha::new("merge-1"),
    }));
    fake.push_is_ancestor(Ok(true));
    fake.push_update_branch_ref(Ok(()));

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    assert_eq!(
        outcome,
        Outcome::Merged {
            merge_sha: WireSha("merge-1".to_owned()),
            fast_forward: false,
            // The integration worktree's own (legacy) `main` checkout is
            // EXCLUDED from the hint — only OTHER worktrees count.
            target_checkout: None,
        }
    );
    // abort-THEN-detach, in that order: an aborted legacy worktree lands
    // back ON the branch, and `checkout --detach` migrates it without moving
    // the branch ref.
    let calls = fake.calls();
    assert_eq!(
        &calls[..4],
        &[
            FakeCall::ResolveBranchTip {
                branch: "main".to_owned(),
            },
            FakeCall::WorktreeStates,
            FakeCall::AbortMerge {
                worktree: integ.clone(),
            },
            FakeCall::DetachWorktree {
                worktree: integ,
                committish: "tip-0".to_owned(),
            },
        ]
    );
    // The legacy-migration branch must STILL reach the CAS: assert the LAST
    // call is the UpdateBranchRef compare-and-swap (mirroring the FF test), so
    // a CAS skip in this branch cannot pass silently.
    assert_eq!(
        calls.last(),
        Some(&FakeCall::UpdateBranchRef {
            branch: "main".to_owned(),
            new: Sha::new("merge-1"),
            expected_old: Sha::new("tip-0"),
            reflog_msg: REFLOG_MSG.to_owned(),
        })
    );
}

#[tokio::test]
async fn merge_hints_when_target_is_checked_out_in_another_worktree() {
    let (exec, fake, root, _guard) = executor("merge-hint");
    let integ = exec.integration_worktree_path("main");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    // The operator's PRIMARY checkout sits on the target branch, dirty —
    // exactly the stale-checkout case the hint exists for.
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("main"), "tip-0", WorktreeStatus::Dirty),
        wt(&integ, None, "tip-0", WorktreeStatus::Clean),
    ]));
    fake.push_detach_worktree(Ok(Sha::new("tip-0")));
    fake.push_head_of(Ok(Sha::new("tip-0")));
    fake.push_merge(Ok(MergeResult::Merged {
        merge_sha: Sha::new("merge-1"),
    }));
    fake.push_is_ancestor(Ok(true));
    fake.push_update_branch_ref(Ok(()));

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    assert_eq!(
        outcome,
        Outcome::Merged {
            merge_sha: WireSha("merge-1".to_owned()),
            fast_forward: false,
            target_checkout: Some(TargetCheckoutHint {
                path: root.display().to_string(),
                dirty: true,
            }),
        }
    );
}

#[tokio::test]
async fn merge_fails_not_found_when_target_branch_is_missing() {
    // The up-front tip resolution is the clean early NotFound — nothing else
    // runs when the target branch does not exist.
    let (exec, fake, _root, _guard) = executor("merge-no-target");
    fake.push_resolve_branch_tip(Err(GitError::NotFound(
        "resolve_branch_tip: branch 'main' does not exist".to_owned(),
    )));

    let outcome = exec.execute(merge_intent(&[])).await;
    assert!(matches!(
        outcome,
        Outcome::Failed {
            kind: FailureKind::NotFound,
            ..
        }
    ));
    assert_eq!(
        fake.calls(),
        vec![FakeCall::ResolveBranchTip {
            branch: "main".to_owned(),
        }]
    );
}

#[tokio::test]
async fn merge_fails_internal_when_integration_head_diverges_from_the_tip() {
    // The executor-side sanity guard (the replacement for the old in-merge
    // checked-out-branch guard): HEAD must sit exactly at the resolved tip.
    let (exec, fake, root, _guard) = executor("merge-head-divergence");
    let integ = exec.integration_worktree_path("main");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, None, "tip-0", WorktreeStatus::Clean),
    ]));
    fake.push_detach_worktree(Ok(Sha::new("tip-0")));
    fake.push_head_of(Ok(Sha::new("somewhere-else")));

    let outcome = exec.execute(merge_intent(&[])).await;
    assert!(matches!(
        outcome,
        Outcome::Failed {
            kind: FailureKind::Internal,
            ..
        }
    ));
    // Refused before the merge ran.
    assert!(matches!(fake.calls().last(), Some(FakeCall::HeadOf { .. })));
}

// --- merge backend-error early-exits (executor.rs steps 2/3) --------------

#[tokio::test]
async fn merge_fails_when_worktree_states_errors() {
    // (executor.rs step 2): the ground-truth observation errors right after
    // the tip resolve — nothing past it runs.
    let (exec, fake, _root, _guard) = executor("merge-states-err");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    fake.push_worktree_states(Err(GitError::Engine(
        "worktree list --porcelain: fatal".to_owned(),
    )));

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    assert!(matches!(
        outcome,
        Outcome::Failed {
            kind: FailureKind::GitFailure,
            ..
        }
    ));
    assert_eq!(
        fake.calls(),
        vec![
            FakeCall::ResolveBranchTip {
                branch: "main".to_owned(),
            },
            FakeCall::WorktreeStates,
        ]
    );
}

#[tokio::test]
async fn merge_fails_internal_when_attach_detached_errors() {
    // (executor.rs step 2, None arm): no integration worktree yet, so the
    // executor attaches a detached one — and that fails. By construction there
    // is no BranchInUse here; an occupied path is companion-internal Internal.
    let (exec, fake, root, _guard) = executor("merge-attach-err");
    let integ = exec.integration_worktree_path("main");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    fake.push_worktree_states(Ok(vec![wt(
        &root,
        Some("dev"),
        "dev-tip",
        WorktreeStatus::Clean,
    )]));
    fake.push_attach_worktree_detached(Err(GitError::State(
        "worktree add --detach: path already exists".to_owned(),
    )));

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    assert!(matches!(
        outcome,
        Outcome::Failed {
            kind: FailureKind::Internal,
            ..
        }
    ));
    // No merge, no further choreography after the failed attach.
    assert_eq!(
        fake.calls(),
        vec![
            FakeCall::ResolveBranchTip {
                branch: "main".to_owned(),
            },
            FakeCall::WorktreeStates,
            FakeCall::AttachWorktreeDetached {
                path: integ,
                committish: "tip-0".to_owned(),
            },
        ]
    );
}

#[tokio::test]
async fn merge_fails_when_detach_errors() {
    // (executor.rs step 2, Some arm): the integration worktree exists and is
    // re-pinned via detach — which errors. `State` here maps to GitFailure.
    let (exec, fake, root, _guard) = executor("merge-detach-err");
    let integ = exec.integration_worktree_path("main");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, None, "tip-old", WorktreeStatus::Clean),
    ]));
    fake.push_detach_worktree(Err(GitError::State(
        "checkout --detach: refusing to lose changes".to_owned(),
    )));

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    assert!(matches!(
        outcome,
        Outcome::Failed {
            kind: FailureKind::GitFailure,
            ..
        }
    ));
    // No merge, no head_of after the failed detach.
    assert_eq!(
        fake.calls(),
        vec![
            FakeCall::ResolveBranchTip {
                branch: "main".to_owned(),
            },
            FakeCall::WorktreeStates,
            FakeCall::DetachWorktree {
                worktree: integ,
                committish: "tip-0".to_owned(),
            },
        ]
    );
}

#[tokio::test]
async fn merge_fails_when_head_of_errors() {
    // (executor.rs step 3): the sanity guard's head_of read errors — refused
    // before the merge runs. `Engine` maps to GitFailure.
    let (exec, fake, root, _guard) = executor("merge-head-err");
    let integ = exec.integration_worktree_path("main");
    fake.push_resolve_branch_tip(Ok(Sha::new("tip-0")));
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, None, "tip-0", WorktreeStatus::Clean),
    ]));
    fake.push_detach_worktree(Ok(Sha::new("tip-0")));
    fake.push_head_of(Err(GitError::Engine("rev-parse HEAD: fatal".to_owned())));

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    assert!(matches!(
        outcome,
        Outcome::Failed {
            kind: FailureKind::GitFailure,
            ..
        }
    ));
    // Refused at the guard — no merge, no CAS.
    let calls = fake.calls();
    assert!(matches!(calls.last(), Some(FakeCall::HeadOf { .. })));
    assert!(
        !calls.iter().any(|c| matches!(c, FakeCall::Merge { .. })),
        "a failed sanity guard must never merge, got {calls:?}"
    );
}

// --- exclude-registration failure (executor.rs steps create/merge) --------

#[tokio::test]
async fn create_worktree_fails_internal_when_git_is_a_file() {
    // The linked-worktree shape (module doc: `.git` is a FILE): the exclude
    // registration cannot create `.git/info/` through a `.git` file, so it
    // fails cleanly as Internal BEFORE any backend call.
    let (exec, fake, _root, _guard) = executor_with_git_file("create-git-file");

    let outcome = exec
        .execute(Intent::CreateWorktree {
            branch: "sprint/serene-1".to_owned(),
            base: "main".to_owned(),
        })
        .await;
    let Outcome::Failed { kind, message } = outcome else {
        panic!("expected Failed, got {outcome:?}");
    };
    assert_eq!(kind, FailureKind::Internal);
    assert!(
        message.contains(".git/info/exclude"),
        "missing exclude-registration context: {message}"
    );
    // The exclude registration runs FIRST — no backend call happened.
    assert_eq!(fake.calls(), vec![]);
}

#[tokio::test]
async fn merge_fails_internal_when_git_is_a_file() {
    // The same exclude-registration failure on the merge path (executor.rs
    // merge step 0): a `.git` FILE fails cleanly as Internal before the tip
    // resolve, naming the exclude registration.
    let (exec, fake, _root, _guard) = executor_with_git_file("merge-git-file");

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    let Outcome::Failed { kind, message } = outcome else {
        panic!("expected Failed, got {outcome:?}");
    };
    assert_eq!(kind, FailureKind::Internal);
    assert!(
        message.contains(".git/info/exclude"),
        "missing exclude-registration context: {message}"
    );
    assert_eq!(fake.calls(), vec![]);
}

// --- reconcile -------------------------------------------------------------

#[tokio::test]
async fn reconcile_reports_managed_worktrees_and_primary_head_as_target_tip() {
    let (exec, fake, root, _guard) = executor("reconcile");
    let managed_clean = exec.worktree_path_for_branch("sprint-serene-1");
    let managed_conflicted = exec.integration_worktree_path("main");
    fake.push_worktree_states(Ok(vec![
        // The primary checkout: NOT companion-managed, excluded from the
        // snapshot list (it only contributes target_tip via head_of).
        wt(&root, Some("main"), "tip-9", WorktreeStatus::Dirty),
        wt(
            &managed_clean,
            Some("sprint-serene-1"),
            "head-1",
            WorktreeStatus::Clean,
        ),
        // A DETACHED managed record (branch: None — the integration
        // worktree's normal state under the detached choreography) flows
        // through the snapshot untouched.
        wt(&managed_conflicted, None, "head-2", WorktreeStatus::Conflicted),
    ]));
    fake.push_head_of(Ok(Sha::new("tip-9")));

    let outcome = exec.execute(Intent::Reconcile).await;
    assert_eq!(
        outcome,
        Outcome::Reconciled {
            worktrees: vec![
                WorktreeSnapshot {
                    path: managed_clean.display().to_string(),
                    branch: Some("sprint-serene-1".to_owned()),
                    head: WireSha("head-1".to_owned()),
                    dirty: false,
                },
                WorktreeSnapshot {
                    path: managed_conflicted.display().to_string(),
                    branch: None,
                    head: WireSha("head-2".to_owned()),
                    dirty: true, // Conflicted folds into the wire dirty flag
                },
            ],
            target_tip: WireSha("tip-9".to_owned()),
        }
    );
    assert_eq!(
        fake.calls(),
        vec![
            FakeCall::WorktreeStates,
            FakeCall::HeadOf {
                worktree: root.clone(),
            },
        ]
    );
}

// --- the exclude-registration helper ---------------------------------------

#[test]
fn exclude_registration_is_append_only_and_idempotent() {
    let (_guard, root) = temp_repo_root("exclude");
    let exclude = root.join(".git").join("info").join("exclude");
    // Pre-existing content WITHOUT a trailing newline: the helper must keep
    // it intact and still land the entry on its own line.
    std::fs::write(&exclude, "/target/").unwrap();

    ensure_worktrees_excluded(&root).unwrap();
    ensure_worktrees_excluded(&root).unwrap(); // idempotent second run

    let contents = std::fs::read_to_string(&exclude).unwrap();
    assert_eq!(contents, format!("/target/\n{WORKTREES_EXCLUDE_ENTRY}\n"));
    assert_eq!(contents.matches(WORKTREES_EXCLUDE_ENTRY).count(), 1);
}

#[test]
fn exclude_registration_creates_a_missing_exclude_file() {
    let (_guard, root) = temp_repo_root("exclude-fresh");
    ensure_worktrees_excluded(&root).unwrap();
    assert_eq!(
        exclude_contents(&root),
        format!("{WORKTREES_EXCLUDE_ENTRY}\n")
    );
}
