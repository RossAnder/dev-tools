//! lumina-protocol — wire types for the lumina control <-> execution plane
//! (ADR-0006). This crate is the NARROW WAIST between `lumina-server` (the
//! record-only control plane) and `lumina-companion` (the git-executing
//! execution plane): serde + serde_json only, no internal deps, no runtime.
//!
//! ## The coarse-protocol rule (ADR-0006 §E)
//!
//! The wire carries coarse INTENTS down and coarse OUTCOMES back — one
//! [`Outcome`] per [`Intent`], full stop. The fine-grained merge loop
//! (resolve / continue / abort, conflict-by-conflict interaction) NEVER
//! crosses the wire: the companion drives it internally against its
//! `GitBackend` and reports a single terminal outcome (e.g.
//! [`Outcome::Conflicted`] after it has already aborted and restored the
//! worktree). Keep it that way — widening the protocol to a chatty
//! per-conflict dialogue is the design this crate exists to forbid.
//!
//! ## Heartbeat
//!
//! Liveness rides WebSocket Ping/Pong FRAMES, not JSON messages. No
//! heartbeat variant exists (or should exist) in these enums.
//!
//! ## Translation boundaries
//!
//! Protocol types are wire-level — deliberately distinct from BOTH the core
//! domain types and the companion's `GitBackend` types. The server translates
//! core <-> protocol at its edge; the companion translates
//! `GitBackend` <-> protocol at its edge. Neither side leaks its internal
//! types into this crate.

use serde::{Deserialize, Serialize};

/// Version of the wire protocol defined by this crate. The companion sends it
/// in [`CompanionToServer::Hello`]; the server rejects a mismatch at handshake
/// time. Bump on ANY breaking change to the JSON shapes below (the pinned
/// snapshot tests in this crate are the tripwire).
pub const PROTOCOL_VERSION: u32 = 1;

/// Correlation id pairing a [`ServerToCompanion::IntentRequest`] with its
/// [`CompanionToServer::Outcome`]. Allocated by the server, monotonically per
/// connection; opaque to the companion. Serializes as a bare JSON number.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct RequestId(pub u64);

/// A git object id as a hex string (full 40/64-char form preferred; the
/// protocol does not abbreviate). Engine-neutral: just the hex text.
/// Serializes as a bare JSON string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha(pub String);

/// Ground-truth state of one worktree as the companion sees it on disk,
/// reported by [`Outcome::Reconciled`]. This is what lets the server's
/// records be DRIVEN by reality rather than the other way around.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeSnapshot {
    /// Absolute path of the worktree on the companion's host.
    pub path: String,
    /// Checked-out branch name; `None` when HEAD is detached.
    pub branch: Option<String>,
    /// Commit the worktree's HEAD points at.
    pub head: Sha,
    /// `true` when the worktree has uncommitted changes (staged or unstaged).
    pub dirty: bool,
}

/// Operator hint riding [`Outcome::Merged`]: the merge TARGET branch was
/// checked out in another worktree (typically the operator's primary
/// checkout) when the branch ref was advanced. That checkout is now STALE
/// relative to the advanced ref — `git status` there shows spurious diffs —
/// and the remedy is `git reset --keep <merge_sha>` run in that worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetCheckoutHint {
    /// Absolute path of the worktree the target branch is checked out in.
    pub path: String,
    /// `true` when that checkout has uncommitted changes (staged or
    /// unstaged) — surface extra caution before any reset.
    pub dirty: bool,
}

/// A coarse git operation the server asks the companion to perform. Each
/// intent resolves to exactly one [`Outcome`].
///
/// Worktree layout is companion-owned: the server never dictates filesystem
/// paths at creation time — it learns them from [`Outcome::WorktreeCreated`] /
/// [`Outcome::Reconciled`] and echoes them back in later intents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Intent {
    /// Create a worktree on a NEW branch `branch` starting at `base`. The
    /// base is any COMMITTISH string (a full SHA, a branch name like `main`,
    /// `HEAD~2`, a tag, …): the COMPANION resolves it against the repository
    /// — the record-only server cannot resolve refs, so it passes the string
    /// through verbatim. [`Outcome::WorktreeCreated`] reports the RESOLVED
    /// `head` SHA back. Success: [`Outcome::WorktreeCreated`].
    CreateWorktree { branch: String, base: String },
    /// Remove the worktree at `path` (as previously reported by the
    /// companion). A dirty worktree fails with
    /// [`FailureKind::DirtyWorktree`] unless `force` is set.
    /// Success: [`Outcome::WorktreeRemoved`].
    RemoveWorktree { path: String, force: bool },
    /// Commit a checkpoint in the worktree at `path` with `message`. A
    /// checkpoint stages ALL modifications in the worktree, untracked files
    /// included (commit-all semantics) — selective staging never crosses the
    /// wire in v1; a future protocol version may reintroduce it. Success:
    /// [`Outcome::Checkpointed`]; nothing-to-commit reports
    /// [`Outcome::AlreadyUpToDate`] with the worktree's current HEAD.
    CommitCheckpoint { path: String, message: String },
    /// Merge `source_branch` into `target_branch`. Every SHA in
    /// `must_remain_reachable` must stay reachable from the target tip after
    /// the merge, else the companion refuses with
    /// [`FailureKind::ReachabilityViolation`] (the guard behind the
    /// `record_worktree_merge` ground-truth inversion). `no_ff` forces a
    /// merge commit even when fast-forward is possible. Success:
    /// [`Outcome::Merged`] | [`Outcome::AlreadyUpToDate`] |
    /// [`Outcome::Conflicted`].
    MergeWorktree {
        source_branch: String,
        target_branch: String,
        must_remain_reachable: Vec<Sha>,
        no_ff: bool,
    },
    /// Report ground truth: every worktree the companion manages plus the
    /// integration target's current tip. Success: [`Outcome::Reconciled`].
    Reconcile,
}

/// The single coarse result of one [`Intent`]. Engine-neutral by design —
/// nothing here presumes shell-git, so a future libgit2/gitoxide backend
/// changes no wire shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outcome {
    /// [`Intent::CreateWorktree`] succeeded. `path` is the companion-chosen
    /// worktree location the server must echo in later intents.
    WorktreeCreated {
        path: String,
        branch: String,
        head: Sha,
    },
    /// [`Intent::RemoveWorktree`] succeeded.
    WorktreeRemoved,
    /// [`Intent::CommitCheckpoint`] produced a commit.
    Checkpointed { commit_sha: Sha },
    /// [`Intent::MergeWorktree`] produced a merge: `merge_sha` is the new
    /// target tip; `fast_forward` is `true` when no merge commit was created.
    /// `target_checkout` is an OPTIONAL operator hint, present when the
    /// target branch was checked out in another worktree at merge time (that
    /// checkout is now stale relative to the advanced ref — see
    /// [`TargetCheckoutHint`]). `#[serde(default)]` keeps frames without the
    /// field deserialising to `None`, and a `None` hint is omitted on
    /// serialise, so a hint-less frame is byte-identical to the prior shape.
    Merged {
        merge_sha: Sha,
        fast_forward: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_checkout: Option<TargetCheckoutHint>,
    },
    /// No-op success: the target already contained the source
    /// ([`Intent::MergeWorktree`]) or there was nothing to commit
    /// ([`Intent::CommitCheckpoint`]). `tip` is the relevant unchanged HEAD.
    AlreadyUpToDate { tip: Sha },
    /// [`Intent::MergeWorktree`] hit conflicts in `paths`. Per the
    /// coarse-protocol rule the companion has ALREADY aborted the merge and
    /// restored the worktree before reporting — no resolve loop follows.
    Conflicted { paths: Vec<String> },
    /// Any intent failed. `kind` is the engine-neutral category; `message`
    /// carries human-readable detail (e.g. trimmed git stderr).
    Failed { kind: FailureKind, message: String },
    /// [`Intent::Reconcile`] ground truth: all managed worktrees plus the
    /// integration target's current tip.
    Reconciled {
        worktrees: Vec<WorktreeSnapshot>,
        target_tip: Sha,
    },
}

/// Engine-neutral failure categories for [`Outcome::Failed`]. Deliberately
/// coarse: the server branches on these, never on git stderr text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// The worktree has uncommitted changes that block the operation.
    DirtyWorktree,
    /// The branch is checked out elsewhere or otherwise locked.
    BranchInUse,
    /// A named path, branch, or SHA does not exist.
    NotFound,
    /// The merge would leave a `must_remain_reachable` SHA unreachable.
    ReachabilityViolation,
    /// target branch ref moved or was deleted between tip-resolve and the
    /// CAS advance; re-run to retry against the new tip.
    TargetMoved,
    /// The git engine reported an error not covered by a finer category.
    GitFailure,
    /// A companion-internal error (I/O, executor, bug) — not a git verdict.
    Internal,
}

/// Messages the server pushes to the companion over the WebSocket. Heartbeat
/// is NOT here — it rides WS Ping/Pong frames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerToCompanion {
    /// Ask the companion to perform `intent`; it answers with exactly one
    /// [`CompanionToServer::Outcome`] carrying the same `id`.
    IntentRequest { id: RequestId, intent: Intent },
}

/// Messages the companion sends to the server over the WebSocket. The
/// companion DIALS the server, so `Hello` is always the first message on a
/// fresh connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CompanionToServer {
    /// Handshake: sent once, immediately after connecting. The server rejects
    /// a `protocol_version` mismatch with [`PROTOCOL_VERSION`].
    Hello {
        protocol_version: u32,
        companion_id: String,
        repo_root: String,
    },
    /// The single coarse result of the intent the server requested as `id`.
    Outcome { id: RequestId, outcome: Outcome },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip<T>(value: &T)
    where
        T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).expect("serialize");
        let back: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&back, value, "round-trip mismatch via {json}");
    }

    fn sha(s: &str) -> Sha {
        Sha(s.to_string())
    }

    fn sample_snapshot() -> WorktreeSnapshot {
        WorktreeSnapshot {
            path: "/work/repo/.worktrees/sprint-1".to_string(),
            branch: Some("sprint/serene-1".to_string()),
            head: sha("0123abcd"),
            dirty: false,
        }
    }

    fn sample_hint() -> TargetCheckoutHint {
        TargetCheckoutHint {
            path: "/work/repo".to_string(),
            dirty: true,
        }
    }

    #[test]
    fn scalar_types_roundtrip() {
        roundtrip(&RequestId(0));
        roundtrip(&RequestId(u64::MAX));
        roundtrip(&sha("deadbeef"));
        roundtrip(&sample_snapshot());
        // Detached-HEAD snapshot: branch is None.
        roundtrip(&WorktreeSnapshot {
            branch: None,
            dirty: true,
            ..sample_snapshot()
        });
        roundtrip(&sample_hint());
    }

    #[test]
    fn every_intent_variant_roundtrips() {
        let intents = vec![
            // `base` is any committish, resolved companion-side: a branch
            // name, a relative rev, or a full SHA all ride the wire verbatim.
            Intent::CreateWorktree {
                branch: "sprint/serene-1".to_string(),
                base: "main".to_string(),
            },
            Intent::CreateWorktree {
                branch: "sprint/serene-2".to_string(),
                base: "HEAD~2".to_string(),
            },
            Intent::CreateWorktree {
                branch: "sprint/serene-3".to_string(),
                base: "0123abcd".to_string(),
            },
            Intent::RemoveWorktree {
                path: "/work/repo/.worktrees/sprint-1".to_string(),
                force: true,
            },
            Intent::CommitCheckpoint {
                path: "/work/repo/.worktrees/sprint-1".to_string(),
                message: "checkpoint: batch 2".to_string(),
            },
            Intent::MergeWorktree {
                source_branch: "sprint/serene-1".to_string(),
                target_branch: "main".to_string(),
                must_remain_reachable: vec![sha("0123abcd"), sha("feedc0de")],
                no_ff: true,
            },
            Intent::Reconcile,
        ];
        for intent in &intents {
            roundtrip(intent);
        }
    }

    #[test]
    fn every_outcome_variant_roundtrips() {
        let outcomes = vec![
            Outcome::WorktreeCreated {
                path: "/work/repo/.worktrees/sprint-1".to_string(),
                branch: "sprint/serene-1".to_string(),
                head: sha("0123abcd"),
            },
            Outcome::WorktreeRemoved,
            Outcome::Checkpointed {
                commit_sha: sha("feedc0de"),
            },
            Outcome::Merged {
                merge_sha: sha("feedc0de"),
                fast_forward: false,
                target_checkout: None,
            },
            Outcome::Merged {
                merge_sha: sha("feedc0de"),
                fast_forward: true,
                target_checkout: Some(sample_hint()),
            },
            Outcome::AlreadyUpToDate {
                tip: sha("0123abcd"),
            },
            Outcome::Conflicted {
                paths: vec!["src/lib.rs".to_string(), "Cargo.toml".to_string()],
            },
            Outcome::Failed {
                kind: FailureKind::DirtyWorktree,
                message: "worktree has uncommitted changes".to_string(),
            },
            Outcome::Reconciled {
                worktrees: vec![sample_snapshot()],
                target_tip: sha("0123abcd"),
            },
        ];
        for outcome in &outcomes {
            roundtrip(outcome);
        }
    }

    #[test]
    fn every_failure_kind_roundtrips() {
        let kinds = [
            FailureKind::DirtyWorktree,
            FailureKind::BranchInUse,
            FailureKind::NotFound,
            FailureKind::ReachabilityViolation,
            FailureKind::TargetMoved,
            FailureKind::GitFailure,
            FailureKind::Internal,
        ];
        for kind in kinds {
            roundtrip(&kind);
            // Also exercise each kind inside the envelope it actually rides in.
            roundtrip(&CompanionToServer::Outcome {
                id: RequestId(1),
                outcome: Outcome::Failed {
                    kind,
                    message: "detail".to_string(),
                },
            });
        }
    }

    /// Back-compat guarantee for [`Outcome::Merged`]'s `#[serde(default)]`
    /// `target_checkout`: a frame predating the hint — the field ABSENT, not
    /// `null` — still deserialises, with the hint as `None`.
    #[test]
    fn merged_without_target_checkout_deserialises_to_none() {
        let legacy = r#"{"type":"merged","merge_sha":"feedc0de","fast_forward":true}"#;
        let outcome: Outcome = serde_json::from_str(legacy).expect("legacy frame deserialises");
        assert_eq!(
            outcome,
            Outcome::Merged {
                merge_sha: sha("feedc0de"),
                fast_forward: true,
                target_checkout: None,
            }
        );
    }

    #[test]
    fn every_envelope_variant_roundtrips() {
        roundtrip(&ServerToCompanion::IntentRequest {
            id: RequestId(7),
            intent: Intent::Reconcile,
        });
        roundtrip(&CompanionToServer::Hello {
            protocol_version: PROTOCOL_VERSION,
            companion_id: "companion-1".to_string(),
            repo_root: "/work/repo".to_string(),
        });
        roundtrip(&CompanionToServer::Outcome {
            id: RequestId(7),
            outcome: Outcome::Reconciled {
                worktrees: vec![sample_snapshot()],
                target_tip: sha("0123abcd"),
            },
        });
    }

    // Pinned-JSON snapshots: the exact wire bytes for one representative
    // message per direction. If one of these fails, you have broken the wire
    // — bump PROTOCOL_VERSION and coordinate both sides before re-pinning.

    #[test]
    fn pinned_wire_json_server_to_companion() {
        let msg = ServerToCompanion::IntentRequest {
            id: RequestId(7),
            intent: Intent::MergeWorktree {
                source_branch: "sprint/serene-1".to_string(),
                target_branch: "main".to_string(),
                must_remain_reachable: vec![sha("0123abcd")],
                no_ff: true,
            },
        };
        let pinned = r#"{"type":"intent_request","id":7,"intent":{"type":"merge_worktree","source_branch":"sprint/serene-1","target_branch":"main","must_remain_reachable":["0123abcd"],"no_ff":true}}"#;
        assert_eq!(serde_json::to_string(&msg).unwrap(), pinned);
        assert_eq!(serde_json::from_str::<ServerToCompanion>(pinned).unwrap(), msg);
    }

    #[test]
    fn pinned_wire_json_companion_to_server() {
        let hello = CompanionToServer::Hello {
            protocol_version: 1,
            companion_id: "companion-1".to_string(),
            repo_root: "/work/repo".to_string(),
        };
        let pinned_hello = r#"{"type":"hello","protocol_version":1,"companion_id":"companion-1","repo_root":"/work/repo"}"#;
        assert_eq!(serde_json::to_string(&hello).unwrap(), pinned_hello);
        assert_eq!(
            serde_json::from_str::<CompanionToServer>(pinned_hello).unwrap(),
            hello
        );

        // A hint-less Merged frame: `skip_serializing_if` keeps these bytes
        // IDENTICAL to the pre-target_checkout wire shape.
        let outcome = CompanionToServer::Outcome {
            id: RequestId(7),
            outcome: Outcome::Merged {
                merge_sha: sha("feedc0de"),
                fast_forward: false,
                target_checkout: None,
            },
        };
        let pinned_outcome = r#"{"type":"outcome","id":7,"outcome":{"type":"merged","merge_sha":"feedc0de","fast_forward":false}}"#;
        assert_eq!(serde_json::to_string(&outcome).unwrap(), pinned_outcome);
        assert_eq!(
            serde_json::from_str::<CompanionToServer>(pinned_outcome).unwrap(),
            outcome
        );

        // A Merged frame carrying the operator hint.
        let outcome_with_hint = CompanionToServer::Outcome {
            id: RequestId(8),
            outcome: Outcome::Merged {
                merge_sha: sha("feedc0de"),
                fast_forward: false,
                target_checkout: Some(sample_hint()),
            },
        };
        let pinned_with_hint = r#"{"type":"outcome","id":8,"outcome":{"type":"merged","merge_sha":"feedc0de","fast_forward":false,"target_checkout":{"path":"/work/repo","dirty":true}}}"#;
        assert_eq!(
            serde_json::to_string(&outcome_with_hint).unwrap(),
            pinned_with_hint
        );
        assert_eq!(
            serde_json::from_str::<CompanionToServer>(pinned_with_hint).unwrap(),
            outcome_with_hint
        );
    }
}
