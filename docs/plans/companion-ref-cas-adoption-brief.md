# Brief: detached-integration ref-CAS + companion adoption pass

> Handoff document for a fresh `/plan-new` session, written 2026-06-10 after ADR-0006
> Step 1b landed. Feed this to the planner; it carries the decisions already made and
> the ground-truth pointers so exploration can stay narrow.

## Context

ADR-0006 Step 1b (flow `serene-jumping-kitten`, status `review`, commits `2433bcc..118e4a2`)
landed the git-execution companion: `lumina-protocol` wire types, `lumina-companion`
(GitBackend trait + ShellGit + executor + WS dial loop), the server companion seam
(`CompanionRegistry`, in-memory per-target-branch merge lease, `GET /api/companion/ws`),
the `execute_worktree_merge` MCP tool + HTTP mirror (count gate = 85), `serve
--with-companion`, and the cross-plane e2e. Read first:

- `docs/plans/serene-jumping-kitten.md` (esp. User Decisions + Risks) and its flow record
  (`.claude/flows/serene-jumping-kitten/PROGRESS-LOG.md` — 3 recorded deviations)
- `docs/adr/0006-git-execution-companion.md`, ADR-0002/0005 (worktree = merge unit;
  one sprint OWNS a worktree; follow-ups TARGET it)
- `lumina/CLAUDE.md` § MCP tool surface (Step-1b paragraph) + § Companion execution
- `lumina/companion/src/executor.rs` (merge choreography), `lumina/companion/src/git/mod.rs`
  (trait — widened once between waves with `attach_worktree`, precedent E11),
  `lumina/server/src/mcp/worktrees.rs` (`execute_worktree_merge_flow`, `LeaseGuard`)

This brief proposes ONE plan with two workstreams (A is the pre-step; B depends on it).

## Workstream A: detached integration worktree + ref-CAS

**Problem (proven in the e2e and in scenario analysis):** the companion's integration
worktree (`.lumina/worktrees/integration-<target>`) CHECKS OUT the target branch, and git
forbids a branch checked out in two worktrees. Any merge whose target (`main`,
`feature/auth`, …) is checked out in the operator's primary checkout — the common case —
fails `Failed{BranchInUse}`. Interim convention: humans/sprints sit only on leaf branches.

**Fix:** the integration worktree becomes a DETACHED checkout of the target tip. Merge
choreography: resolve target tip → detach integration worktree there → merge source (a
merge in detached HEAD produces the merge commit on HEAD) → reachability gate (unchanged,
ADR §H) → advance the branch ref with compare-and-swap:
`git update-ref refs/heads/<target> <merge-sha> <expected-old-tip>`. CAS failure ("target
moved underneath me") is a NEW coarse failure outcome — strictly better than today's
semantics. No branch is ever checked out, so the `BranchInUse` class disappears for merges.

**Touches:** `executor.rs` choreography + tests; `git/mod.rs` trait widening (≈
`resolve_branch_tip`, `update_branch_ref(target, new, expected_old)`, a detached-attach
variant alongside `attach_worktree`); `shell.rs` + `fake.rs` impls + `shell_git.rs` tests;
`companion_e2e.rs` (drop the detach-the-primary fixture workaround — it becomes the
regression test: merge succeeds WHILE the target is checked out in the primary);
possibly `FailureKind` (wire is UNSHIPPED — fields/variants may change without a
version bump, precedent E12; `PROTOCOL_VERSION` stays 1).

## Workstream B: adoption pass (lifecycle skills drive the companion)

`execute_worktree_merge` has zero callers. Teach the dogfood lifecycle to use it:

- `/lumina:run-sprint` + `/lumina:compose-sprint` + the lifecycle advisor
  (`claude/plugins/lumina-story-blocks/skills/...`) and
  `lumina/docs/runbooks/dogfood-lifecycle.md`: prefer `execute_worktree_merge` over
  manual `git merge` + `record_worktree_merge`; own the same-target sequencing loop
  (lease rejects with "already in flight" — it does not queue; retry in dependency order).
- Encode the lifecycle invariants: a worktree merges EXACTLY ONCE (pre-flight requires
  the OWNING sprint in `review`; post-merge it is `done`); follow-up work after a merge =
  NEW sprint + NEW worktree off the updated base; integration/target branches are never
  checked out by humans or sprints (moot for merges once A lands).
- Decide during planning (Phase-4 directed questions):
  1. Un-park the `CreateWorktree` public trigger (User Decision 4 deferred it) so the
     composer can mint sprint worktrees through the companion instead of manual git?
  2. Pull in the deferred UNIQUE live-branch index (migration 0018 is next free) now that
     the composer would mint branches — or keep deferring?
  3. One flow or two? (Default: one plan, A as wave 1, B after.)

## Known facts to carry (do not rediscover)

- Lease: in-memory, keyed by exact target-branch string, voided on disconnect/restart;
  re-runs are idempotent via `AlreadyUpToDate` + ground-truth-SHA record. Reconnect
  auto-`Reconcile` is logged only — it does not reconcile the store.
- Companion executes intents SEQUENTIALLY (one in flight) — different-target merges queue
  briefly; accepted 1b simplification, out of scope here.
- `must_remain_reachable` derives from `repo::list_worktree_reachable_shas` (UNION over
  both `task_commits` join paths — nullable `sprint_id` trap is handled).
- Integration-worktree dir names sanitise `/`→`-` (`feature/auth` → `integration-feature-auth`);
  avoid branch names that collide post-sanitisation.
- GitBackend trait-gap protocol: gaps surface to the orchestrator and are fixed BETWEEN
  waves (never mid-wave); two precedents in the Step-1b flow record (E11, E12).
- Repo build discipline: sub-agents run `cargo clippy --manifest-path lumina/Cargo.toml`
  + narrow `cargo nextest --profile quick -E '...'` only; full build/test/lint belongs to
  the orchestrator's verification tiers.

## Verification (unchanged from Step 1b)

```
build: cargo build --workspace --manifest-path lumina/Cargo.toml
test:  cargo nextest run --manifest-path lumina/Cargo.toml --profile ci
lint:  cargo clippy --workspace --manifest-path lumina/Cargo.toml --all-targets
smoke: cargo tree -p lumina-server -e normal | rg -i '\b(git2|gix)'              # zero
       cargo tree -p lumina-companion -e normal | rg 'lumina-(core|server)|sqlx|axum'  # zero
```

Plus, for A: the new e2e scenario — merge succeeds while the target branch is checked out
in the primary checkout; CAS-failure path covered against `FakeGitBackend`.
