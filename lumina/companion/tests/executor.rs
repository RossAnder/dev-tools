//! Executor intent->outcome tests against the scripted [`FakeGitBackend`] —
//! deterministic, no git binary, no real repository. The only filesystem
//! touch is the `.git/info/exclude` registration, exercised against a
//! fabricated `<tempdir>/.git/info/` per test.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lumina_companion::executor::{
    Executor, WORKTREES_EXCLUDE_ENTRY, ensure_worktrees_excluded,
};
use lumina_companion::git::{
    GitError, MergeResult, Sha, WorktreeState, WorktreeStatus,
    fake::{FakeCall, FakeGitBackend},
};
use lumina_protocol::{FailureKind, Intent, Outcome, Sha as WireSha, WorktreeSnapshot};

/// A fresh fabricated repo root: `<temp>/.git/info/` exists, nothing else.
/// Removed up front so a stale dir from a prior run can't pollute the
/// exclude-file assertions.
fn temp_repo_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "lumina-companion-executor-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".git").join("info")).unwrap();
    root
}

/// Executor over a fake backend, keeping the concrete handle for call-log
/// assertions (the `Arc<dyn GitBackend>` consumption shape).
fn executor(tag: &str) -> (Executor, Arc<FakeGitBackend>, PathBuf) {
    let root = temp_repo_root(tag);
    let fake = Arc::new(FakeGitBackend::new());
    let exec = Executor::new(root.clone(), fake.clone());
    (exec, fake, root)
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

fn exclude_contents(root: &Path) -> String {
    std::fs::read_to_string(root.join(".git").join("info").join("exclude")).unwrap()
}

// --- the three simple intents -------------------------------------------

#[tokio::test]
async fn create_worktree_translates_and_registers_exclude() {
    let (exec, fake, root) = executor("create");
    let expected_path = exec.worktree_path_for_branch("sprint/serene-1");
    fake.push_create_worktree(Ok(wt(
        &expected_path,
        Some("sprint/serene-1"),
        "head-1",
        WorktreeStatus::Clean,
    )));

    let outcome = exec
        .execute(Intent::CreateWorktree {
            branch: "sprint/serene-1".to_owned(),
            base: WireSha("base-1".to_owned()),
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
        vec![FakeCall::CreateWorktree {
            path: expected_path,
            branch: "sprint/serene-1".to_owned(),
            start_point: Sha::new("base-1"),
        }]
    );
    assert!(exclude_contents(&root).contains(WORKTREES_EXCLUDE_ENTRY));
}

#[tokio::test]
async fn remove_worktree_passes_through_and_maps_dirty_state() {
    let (exec, fake, _root) = executor("remove");
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
    let (exec, fake, _root) = executor("checkpoint");
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
async fn merge_happy_path_runs_gate_and_reports_merged() {
    let (exec, fake, root) = executor("merge-happy");
    let integ = exec.integration_worktree_path("main");
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, Some("main"), "tip-0", WorktreeStatus::Clean),
    ]));
    fake.push_head_of(Ok(Sha::new("tip-0")));
    fake.push_merge(Ok(MergeResult::Merged {
        merge_sha: Sha::new("merge-1"),
    }));
    fake.push_is_ancestor(Ok(true));
    fake.push_is_ancestor(Ok(true));

    let outcome = exec.execute(merge_intent(&["keep-1", "keep-2"])).await;

    assert_eq!(
        outcome,
        Outcome::Merged {
            merge_sha: WireSha("merge-1".to_owned()),
            fast_forward: false,
        }
    );
    assert_eq!(
        fake.calls(),
        vec![
            FakeCall::WorktreeStates,
            FakeCall::HeadOf {
                worktree: integ.clone(),
            },
            FakeCall::Merge {
                worktree: integ,
                source: "sprint/serene-1".to_owned(),
                target: "main".to_owned(),
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
        ]
    );
}

#[tokio::test]
async fn merge_fast_forward_reports_fast_forward_true() {
    let (exec, fake, root) = executor("merge-ff");
    let integ = exec.integration_worktree_path("main");
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, Some("main"), "tip-0", WorktreeStatus::Clean),
    ]));
    fake.push_head_of(Ok(Sha::new("tip-0")));
    fake.push_merge(Ok(MergeResult::FastForward {
        new_tip: Sha::new("ff-1"),
    }));
    fake.push_is_ancestor(Ok(true));

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    assert_eq!(
        outcome,
        Outcome::Merged {
            merge_sha: WireSha("ff-1".to_owned()),
            fast_forward: true,
        }
    );
}

#[tokio::test]
async fn merge_already_up_to_date_maps_through_with_pre_merge_tip() {
    let (exec, fake, root) = executor("merge-noop");
    let integ = exec.integration_worktree_path("main");
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, Some("main"), "tip-0", WorktreeStatus::Clean),
    ]));
    fake.push_head_of(Ok(Sha::new("tip-0")));
    fake.push_merge(Ok(MergeResult::AlreadyUpToDate));
    // NOTE: no is_ancestor scripted — the gate must not run on a no-op.

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    assert_eq!(
        outcome,
        Outcome::AlreadyUpToDate {
            tip: WireSha("tip-0".to_owned()),
        }
    );
}

#[tokio::test]
async fn merge_conflict_aborts_locally_then_reports_conflicted() {
    let (exec, fake, root) = executor("merge-conflict");
    let integ = exec.integration_worktree_path("main");
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, Some("main"), "tip-0", WorktreeStatus::Clean),
    ]));
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
    // outcome was reported.
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
        target: "main".to_owned(),
        no_ff: true,
    }));
}

#[tokio::test]
async fn merge_gate_violation_rolls_back_to_pre_merge_tip() {
    let (exec, fake, root) = executor("merge-gate");
    let integ = exec.integration_worktree_path("main");
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, Some("main"), "tip-0", WorktreeStatus::Clean),
    ]));
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
    // The rollback went through the backend, to the recorded pre-merge tip.
    assert_eq!(
        fake.calls().last(),
        Some(&FakeCall::ResetHard {
            worktree: integ,
            to: Sha::new("tip-0"),
        })
    );
}

#[tokio::test]
async fn merge_refuses_dirty_integration_worktree_before_merging() {
    let (exec, fake, root) = executor("merge-dirty");
    let integ = exec.integration_worktree_path("main");
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, Some("main"), "tip-0", WorktreeStatus::Dirty),
    ]));

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    assert!(matches!(
        outcome,
        Outcome::Failed {
            kind: FailureKind::DirtyWorktree,
            ..
        }
    ));
    // Refused on the ground-truth observation alone — no merge, no head_of.
    assert_eq!(fake.calls(), vec![FakeCall::WorktreeStates]);
}

#[tokio::test]
async fn merge_self_heals_an_interrupted_merge_before_retrying() {
    let (exec, fake, root) = executor("merge-heal");
    let integ = exec.integration_worktree_path("main");
    fake.push_worktree_states(Ok(vec![
        wt(&root, Some("dev"), "dev-tip", WorktreeStatus::Clean),
        wt(&integ, Some("main"), "tip-0", WorktreeStatus::Conflicted),
    ]));
    fake.push_abort_merge(Ok(())); // the self-heal
    fake.push_head_of(Ok(Sha::new("tip-0")));
    fake.push_merge(Ok(MergeResult::Merged {
        merge_sha: Sha::new("merge-1"),
    }));
    fake.push_is_ancestor(Ok(true));

    let outcome = exec.execute(merge_intent(&["keep-1"])).await;
    assert_eq!(
        outcome,
        Outcome::Merged {
            merge_sha: WireSha("merge-1".to_owned()),
            fast_forward: false,
        }
    );
    // abort_merge healed the leftover state BEFORE the new merge attempt.
    let calls = fake.calls();
    assert_eq!(
        &calls[..3],
        &[
            FakeCall::WorktreeStates,
            FakeCall::AbortMerge {
                worktree: integ.clone(),
            },
            FakeCall::HeadOf { worktree: integ },
        ]
    );
}

#[tokio::test]
async fn merge_lazily_attaches_the_integration_worktree() {
    let (exec, fake, root) = executor("merge-lazy");
    let integ = exec.integration_worktree_path("main");
    // No integration worktree yet — and no checkout of `main` anywhere
    // either: attach_worktree checks the EXISTING branch out directly, so no
    // base resolution from another checkout is needed.
    fake.push_worktree_states(Ok(vec![wt(
        &root,
        Some("dev"),
        "dev-tip",
        WorktreeStatus::Clean,
    )]));
    fake.push_attach_worktree(Ok(Sha::new("tip-0")));
    fake.push_head_of(Ok(Sha::new("tip-0")));
    fake.push_merge(Ok(MergeResult::AlreadyUpToDate));

    let outcome = exec.execute(merge_intent(&[])).await;
    assert_eq!(
        outcome,
        Outcome::AlreadyUpToDate {
            tip: WireSha("tip-0".to_owned()),
        }
    );
    assert_eq!(
        fake.calls()[1],
        FakeCall::AttachWorktree {
            branch: "main".to_owned(),
            path: integ,
        }
    );
}

#[tokio::test]
async fn merge_fails_not_found_when_target_branch_is_missing() {
    // No integration worktree and the target branch does not exist: the
    // attach reports NotFound, which maps straight through the kind table.
    let (exec, fake, root) = executor("merge-no-target");
    let integ = exec.integration_worktree_path("main");
    fake.push_worktree_states(Ok(vec![wt(
        &root,
        Some("dev"),
        "dev-tip",
        WorktreeStatus::Clean,
    )]));
    fake.push_attach_worktree(Err(GitError::NotFound(
        "attach_worktree: invalid reference: main".to_owned(),
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
        vec![
            FakeCall::WorktreeStates,
            FakeCall::AttachWorktree {
                branch: "main".to_owned(),
                path: integ,
            },
        ]
    );
}

// --- reconcile -------------------------------------------------------------

#[tokio::test]
async fn reconcile_reports_managed_worktrees_and_primary_head_as_target_tip() {
    let (exec, fake, root) = executor("reconcile");
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
    let root = temp_repo_root("exclude");
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
    let root = temp_repo_root("exclude-fresh");
    ensure_worktrees_excluded(&root).unwrap();
    assert_eq!(
        exclude_contents(&root),
        format!("{WORKTREES_EXCLUDE_ENTRY}\n")
    );
}
