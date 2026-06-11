# Plan: Detached-integration ref-CAS merge + companion adoption pass

**Plan path**: `docs/plans/composed-painting-truffle.md`
**Created**: 2026-06-10
**Status**: Draft

## Context

ADR-0006 Step 1b (flow `serene-jumping-kitten`) landed the git-execution companion: `lumina-protocol` wire types, `lumina-companion` (GitBackend trait + ShellGit + executor + WS dial loop), the server companion seam, and the `execute_worktree_merge` MCP tool + HTTP mirror (count gate 85). Two gaps remain, handed off via `docs/plans/companion-ref-cas-adoption-brief.md`:

1. **Workstream A — the `BranchInUse` wall.** The companion's integration worktree (`.lumina/worktrees/integration-<target>`) checks out the target branch, and git forbids a branch checked out in two worktrees. Any merge whose target is checked out in the operator's primary checkout — the common case — fails `Failed{BranchInUse}`. Fix: make the integration worktree a DETACHED checkout of the target tip and advance the branch ref afterwards with an atomic compare-and-swap (`git update-ref refs/heads/<target> <new> <expected-old>`). No branch is ever checked out by the companion, so the `BranchInUse` class disappears for merges; a lost CAS ("target moved underneath me") becomes a new coarse failure outcome.
2. **Workstream B — zero callers.** `execute_worktree_merge` exists but nothing drives it: the dogfood lifecycle (runbook + `/lumina:run-sprint` / `/lumina:compose-sprint` / lifecycle advisor) still instructs manual `git merge` + `record_worktree_merge`. The adoption pass teaches the lifecycle to prefer the companion execution plane and encodes the lifecycle invariants (merge exactly once; follow-up work = new sprint + new worktree off the updated base).

Per User Decision 2, this plan also un-parks the `CreateWorktree` public trigger (`execute_worktree_create`, count gate 85→86) so the composer can mint sprint worktrees through the companion, and per User Decision 3 adds migration 0018 (partial UNIQUE live-branch index).

## Scope

- **In scope**: companion merge choreography (detached integration worktree + ref-CAS), GitBackend trait widening, protocol wire changes (unshipped, no version bump), `execute_worktree_create` MCP tool + HTTP mirror, migration 0018, e2e regression rewrite, runbook/skills/docs adoption pass.
- **Out of scope**: companion concurrency (intents stay sequential — accepted 1b simplification); reconcile-to-store writes (reconnect `Reconcile` stays log-only); lease persistence (stays in-memory); GitLab/host-qualified repo slugs; any change to the reachability-gate semantics (ADR §H unchanged).
- **Affected areas**: `lumina/protocol/`, `lumina/companion/`, `lumina/server/`, `lumina/core/`, `lumina/migrations/`, `lumina/docs/runbooks/`, `claude/plugins/lumina-story-blocks/`, `docs/adr/0006-git-execution-companion.md`, `lumina/CLAUDE.md`, `CLAUDE.md`.
- **Estimated file count**: ~21 unique files (8 tasks across 4 sequential waves, ≤2 parallel agents per wave, ≤5 files per task). Over the ~15 guideline — explicitly accepted in User Decision 1 (one plan, A then B).

## Research Notes

> Vetted 2026-06-10: Agent-1 (update-ref CAS) 3 sampled / 0 dropped / 0 downgraded; Agent-2 (detached worktrees) 3 sampled / 0 dropped / 1 downgraded; Agent-3 (sqlite-partial-index, Phase 5) 3 sampled / 0 dropped / 0 downgraded. Spot-checks fetched git-scm.com man pages directly. `[[vet_events]]` append deferred to post-bootstrap (no flow ledger exists in plan mode).

### update-ref CAS (Agent-1)

- **3-arg `git update-ref <ref> <new> <old>` is an atomic verify-and-swap under the ref lock** — "after verifying that the current value of the <ref> matches <old-oid>"; no TOCTOU window; `--stdin` transactions only needed for multi-ref updates. [A — https://git-scm.com/docs/git-update-ref] *Impact: single invocation suffices; no extra locking beyond the server's merge lease.*
- **CAS failure is distinctly detectable**: non-zero exit (typically 128) + stderr `error: cannot lock ref '<ref>': is at <actual> but expected <old>`. [B — man page + corroborating discussion] *Impact: ShellGit classifies on the `is at … but expected` substring (LC_ALL=C already pinned) → distinct CAS-lost error, not generic Engine.*
- **update-ref does NOT refuse to move a branch checked out in another worktree** (the guard lives only in porcelain — `git branch -f`, `worktree add`; man page confirmed silent on worktree safety). [B] *Impact: exactly what makes the design work — merges succeed while the operator sits on the target.*
- **Consequence for the operator's primary checkout**: its HEAD symref now resolves to the new tip while index/worktree still reflect the old one — `git status` shows spurious "undo-the-merge" changes; remedy is `git reset --keep <new-tip>` (or `--hard` if clean). [B/C] *Impact: User Decision 4 — runbook note + outcome hint.*
- **`-m <reason>` sets the branch reflog message** (refs/heads/* logged by default); other worktrees' HEAD reflogs untouched. [A] *Impact: pass `-m "lumina-companion: merge <source>"` for the audit trail.*
- **Zero-OID conventions**: 40 zeros as `<old>` = "must not exist" (create-only); omitting `<old>` drops verification entirely. [A] *Impact: always pass the real expected old tip.*

### Detached worktrees + detached-HEAD merges (Agent-2)

- **`git worktree add --detach <path> <commit-ish>` creates a branch-less detached checkout**; the "already checked out" refusal triggers only when `<commit-ish>` is a branch name checked out elsewhere — `--detach` can never trigger it. [A — https://git-scm.com/docs/git-worktree; downgraded nuance: the refusal clause also covers a stale assigned `<path>`, overridden by `--force`]
- **`worktree list --porcelain` emits a bare `detached` label and NO `branch refs/heads/…` line for detached worktrees.** [A] *Impact: the ShellGit parser already tolerates this (`branch = None`, shell.rs ≈610–616, `WorktreeState.branch: Option` — Plan-agent verified); integration-worktree recognition keys on path.*
- **Re-pointing**: `checkout --detach <sha>` fails safe on dirty state; `reset --hard <sha>` forces. [A] *Impact: choreography uses `checkout --detach` — a LEGACY integration worktree still has the target branch checked out, and `reset --hard` there would move the branch ref; `checkout --detach` migrates it safely.*
- **Merging in detached HEAD is fully supported**: merge commit lands ON HEAD, no branch moves; `--no-ff` still forces a true merge commit; conflict/`MERGE_HEAD`/`merge --abort` semantics identical to on-branch merges. [A — https://git-scm.com/docs/git-merge]
- **gc window between merge and ref-advance is protected** by the per-worktree HEAD reflog (`gc.reflogExpire` default 90 days). [A] *Impact: non-issue; advance the ref promptly anyway.*
- **Version floor**: `worktree add --detach` + porcelain `detached` ≈ git 2.7+; `checkout --detach` is the conservative idiom (no new floor). [B]

### Directed research additions (Phase 5 — SQLite partial UNIQUE index, prompted by User Decision 3)

- **`CREATE UNIQUE INDEX … WHERE …` validates existing rows and fails the statement on violation** (`UNIQUE constraint failed: worktrees.branch`); sqlx runs each SQLite migration file in one transaction, so the file is all-or-nothing. [A/B — sqlite.org/lang_createindex.html] *Impact: a dirty dev DB with duplicate live branches makes 0018 fail loudly at startup; dev DB is gitignored/recreatable, so ship index-only and document the failure mode rather than embedding fragile dedupe SQL.*
- **NULLs are pairwise distinct in SQLite UNIQUE indexes** — `branch IS NOT NULL` in the predicate is self-documenting, not load-bearing. [A]
- **Partial-index WHERE allows only same-table columns + deterministic operators** — `outcome IS NULL AND branch IS NOT NULL` is legal; same shape as the existing `repo_links(project_id) WHERE is_primary = 1` precedent (migration 0004). [A — sqlite.org/partialindex.html]
- **A violating write raises `SQLITE_CONSTRAINT_UNIQUE` (extended code 2067) naming `worktrees.branch`, NOT the index name.** [B] *Impact: map via the sqlx error code to a typed Validation error in `repo::create_worktree`; don't string-match an index name that isn't in the message.*
- **Version floor 3.8.0 — comfortably cleared** (repo already ships partial indexes in 0004/0013). [A]

### Codebase ground truth (exploration)

- **Choreography** (`lumina/companion/src/executor.rs` ≈194–339): `merge_worktree` runs worktree_states → lazy `attach_worktree(target, integration_path)` → self-heal Conflicted / refuse Dirty → `head_of` anchor → `merge(integration, source, target, no_ff)` → classify (AlreadyUpToDate / Conflict→abort / FastForward / Merged) → reachability gate (`is_ancestor` per `must_remain_reachable` SHA, rollback via `reset_hard`) → `Outcome::Merged`. Integration path = `.lumina/worktrees/integration-<sanitised>` (`branch_dir_name` ≈399–412); `.git/info/exclude` registration via `ensure_worktrees_excluded`.
- **GitBackend** (`git/mod.rs` ≈206–280, object-safe): create_worktree / attach_worktree (E11) / remove_worktree / commit_all / merge / abort_merge / resolve / is_ancestor / commit_exists / worktree_states / head_of / reset_hard. ShellGit `base_command()` pins `LC_ALL=C` (stderr parsing is load-bearing). FakeGitBackend = per-method FIFO `VecDeque<Result<…>>` + `FakeCall` log, no semantic state.
- **Protocol** (`lumina/protocol/src/lib.rs`): `FailureKind{DirtyWorktree, BranchInUse, NotFound, ReachabilityViolation, GitFailure, Internal}`; `Outcome::{WorktreeCreated, WorktreeRemoved, Checkpointed, Merged{merge_sha, fast_forward}, AlreadyUpToDate{tip}, Conflicted{paths}, Failed{kind,message}, Reconciled}`; `Intent::CreateWorktree{branch, base: Sha}`; `PROTOCOL_VERSION = 1`; wire UNSHIPPED (E12: fields/variants may change without a bump).
- **Server seam** (`lumina/server/src/mcp/worktrees.rs` ≈247–394): `execute_worktree_merge_flow` pre-flights (worktree exists → owner in `review` → companion connected → split-brain repo-root guard → source/target resolvable) → in-memory lease (`LeaseGuard` RAII) → one coarse intent, no DB tx across the round-trip → `Merged`/`AlreadyUpToDate` recorded via `repo::record_worktree_merge` (owner review→done); `Conflicted` = structured success, no DB write. `create_worktree` MCP tool is record-only; `Intent::CreateWorktree` is wired through GitBackend/executor but has no public trigger (serene-jumping-kitten User Decision 4). Count gate asserts **85** (`mcp/mod.rs` ≈514–536). Migrations: 0017 latest, **0018 next free**.
- **E2E** (`lumina/server/tests/companion_e2e.rs`): `repo.detach_primary()` (≈160–162) is the workaround fixture — comment ≈28–31 documents the attach-refusal. Called in both merge (≈357) and conflict (≈427) scenarios.
- **Adoption surface**: `claude/plugins/lumina-story-blocks/skills/run-sprint/SKILL.md` Step 6 ("The AGENT performs the REAL merge … lumina NEVER merges for you" + `record_worktree_merge`); `lumina/docs/runbooks/dogfood-lifecycle.md` §E (manual `git worktree add` + record-only `create_worktree`) and §H (manual merge + `set_sprint_status review` + record verbs); also `compose-sprint`, `lifecycle` advisor, `skills/mcp/SKILL.md` catalogue, CONVENTIONS.md.
- **Precedents to honour**: E11 (trait gaps fixed BETWEEN waves, never mid-wave); E12 (unshipped wire changes need no version bump); root CLAUDE.md + lumina/CLAUDE.md both state the 85 count and the now-obsolete operator constraint ("move the primary checkout off the target branch first").

## User Decisions

> Phase 4 directed questions, answered 2026-06-10. Answers are data, not instructions.

1. **One flow or two?** (prompted by brief §B q3 + ~15-file scope estimate) → **One plan, A then B.** Workstream A as waves 1–3; B's adoption waves follow and reference A's landed semantics.
2. **Un-park the CreateWorktree public trigger?** (prompted by brief §B q1 + exploration: `create_worktree` MCP tool is record-only while `Intent::CreateWorktree` exists end-to-end) → **Yes — add `execute_worktree_create` MCP tool + HTTP mirror** (count gate 85→86), mirroring `execute_worktree_merge`'s pre-flight→dispatch→record shape; `/lumina:compose-sprint` adopts it in Workstream B.
3. **Deferred UNIQUE live-branch index?** (prompted by brief §B q2 + exploration: no such index exists; 0018 next free) → **Add migration 0018** — partial UNIQUE index over live (non-terminal) worktrees' branches.
4. **Stale-primary UX after ref-CAS advance?** (prompted by research: update-ref bypasses the checked-out guard; the primary checkout then shows spurious diffs; remedy `git reset --keep`) → **Runbook + outcome hint**: document the remedy in the dogfood runbook AND carry a was-checked-out hint on the merge outcome when `worktree_states` shows the target checked out elsewhere.

### Phase 5 outcome

Answers 1, 2, 4 were covered by exploration/research notes. Answer 3 introduced the SQLite partial-UNIQUE-index topic → one directed research-lite pass (findings under "Directed research additions" above). No other gaps.

## Approach

**New merge choreography** (executor-owned, mirroring today's step style):

1. `resolve_branch_tip(target)` → `expected_old_tip` (FIRST — clean early `NotFound`, maximal CAS-protected window; the server-side per-target lease serialises companion-driven merges, and the CAS catches operator commits regardless).
2. Ensure a DETACHED integration worktree at `expected_old_tip`: missing → `attach_worktree_detached(path, tip)` (`git worktree add --detach`); present → existing guards (abort stale `Conflicted` FIRST, refuse `Dirty`), THEN `detach_worktree(path, tip)` (`git checkout --detach`) — explicitly after the abort, because an aborted legacy worktree lands back ON the target branch, and `checkout --detach` (never `reset --hard`) is what migrates a legacy on-branch integration worktree without moving the branch ref.
3. Sanity guard in the executor (replaces ShellGit's in-`merge()` checked-out-branch guard): `head_of(integration) == expected_old_tip`, else `Failed{Internal}`. `merge()` shrinks to `(worktree, source, no_ff)`.
4. Merge + classify exactly as today. `AlreadyUpToDate` → return early (HEAD pinned at the tip, "unmoved" ⟺ source already reachable — no CAS, no record divergence). `Conflicted` → abort + return, no ref touched.
5. Reachability gate unchanged (ADR §H), anchored on `expected_old_tip`; on violation `reset_hard` rollback as today — note the gate now runs BEFORE any ref moves, so a failed gate never touched the real branch (strict improvement).
6. **NEW**: `update_branch_ref(target, new_tip, expected_old_tip, reflog_msg)` — atomic CAS. Lost CAS (stderr `is at … but expected`) AND deleted-ref-mid-flight (`unable to resolve`) both classify as the new `FailureKind::TargetMoved`; no rollback needed (no ref was touched; the orphan merge commit is reflog-protected ~90 days and the next run re-detaches).
7. `Outcome::Merged` gains `target_checkout: Option<TargetCheckoutHint{path, dirty}>` (`#[serde(default)]`), derived from the already-fetched `worktree_states()` snapshot (pre-merge; staleness acceptable for a hint) — the server has no git, and `Reconciled` filters to the managed root, so the outcome field is the only channel (Plan-agent verified).

**Key decisions + rejected alternatives**:
- `PROTOCOL_VERSION` stays **1** (brief + E12: wire unshipped). Rejected: bumping to 2 (Plan-agent suggestion) — contradicts the brief's pinned decision; `#[serde(default)]` on new fields adopted instead.
- Two trait methods (`attach_worktree_detached` + `detach_worktree`) rather than one `ensure_detached_worktree` — preserves executor-owned choreography, matching the existing attach/self-heal branching style.
- Migration 0018 ships **index-only** (no pre-dedupe SQL). Rejected: demote-older-duplicates UPDATE — fragile tie-breaks and it would bypass the sprint-transition invariants; a dirty dev DB fails loudly and is recreatable.
- `BranchInUse` variant **stays** (still produced by create paths, e.g. `create_worktree` State mapping at executor.rs ≈159); merges simply stop producing it.
- `Intent::CreateWorktree.base` widens `Sha` → committish `String` — the record-only server cannot resolve refs; the companion resolves (reusing `resolve_commit`/`resolve_branch_tip`).
- `execute_worktree_create` takes no merge lease — the executor is sequential and creation moves no refs.

**Reuse**: `execute_worktree_merge_flow`'s pre-flight helpers (companion-connected, split-brain guard) and the no-DB-tx-across-round-trip shape for the create flow; `branch_dir_name` + `worktree_path_for_branch` for paths; `ensure_worktrees_excluded`; ShellGit's `base_command()`/stderr-classification idiom; FakeGitBackend's FIFO + `FakeCall` pattern; `TestRepo` fixtures in both test suites; migration-0004 partial-UNIQUE-index shape.

## Verification Commands

```
build: cargo build --workspace --manifest-path lumina/Cargo.toml
test:  cargo nextest run --manifest-path lumina/Cargo.toml --profile ci
lint:  cargo clippy --workspace --manifest-path lumina/Cargo.toml --all-targets
smoke: cargo tree --manifest-path lumina/Cargo.toml -p lumina-server -e normal | rg -i '\b(git2|gix)'              # must be empty
       cargo tree --manifest-path lumina/Cargo.toml -p lumina-companion -e normal | rg 'lumina-(core|server)|sqlx|axum'  # must be empty
```

> Build discipline: sub-agents run `cargo clippy --manifest-path lumina/Cargo.toml` + their task's narrow `cargo nextest … --profile quick -E '<filter>'` only; the full build/test/lint/smoke pass belongs to the orchestrator's verification tiers (per-wave checkpoint + final).

## Tasks

### Wave 1 — protocol + schema substrate (parallel)

### 1. Widen the unshipped protocol wire types [M]
- **Files**: `lumina/protocol/src/lib.rs`
- **Depends on**: —
- **Action**: Add `FailureKind::TargetMoved`, add `target_checkout` to `Outcome::Merged`, and widen `Intent::CreateWorktree.base` from `Sha` to a committish `String`.
- **Detail**: `TargetMoved` doc-comment: "target branch ref moved or was deleted between tip-resolve and the CAS advance; re-run to retry against the new tip". New struct `TargetCheckoutHint { path: String, dirty: bool }`; `Outcome::Merged` gains `#[serde(default)] target_checkout: Option<TargetCheckoutHint>` (old frames without the field still deserialise). `CreateWorktree { branch: String, base: String }` — `base` is any committish; doc that the companion resolves it. `PROTOCOL_VERSION` stays 1 (E12 precedent — wire unshipped). Update the in-module serde round-trip/pinned-snapshot tests for all three changes.
- **Acceptance**: `cargo clippy --manifest-path lumina/Cargo.toml -p lumina-protocol --all-targets` clean; `cargo nextest run --manifest-path lumina/Cargo.toml --profile quick -E 'package(lumina-protocol)'` passes.

### 2. Migration 0018 — partial UNIQUE live-branch index [M]
- **Files**: `lumina/migrations/0018_live_branch_unique.sql`, `lumina/core/src/repo/worktrees.rs`, plus the existing worktrees repo-layer test module/file (locate the migration-0016 worktree tests and extend them)
- **Depends on**: —
- **Action**: Add `CREATE UNIQUE INDEX idx_worktrees_live_branch ON worktrees(branch) WHERE outcome IS NULL AND branch IS NOT NULL;` as a new forward-only migration, and map the violation to a typed error in `repo::create_worktree`.
- **Detail**: Index-only migration — NO dedupe SQL (a dirty dev DB fails loudly at startup; document in the migration header comment that the remedy is resolving duplicates via `record_worktree_rejection` or recreating the gitignored dev DB). In `repo::create_worktree`, detect `SQLITE_CONSTRAINT_UNIQUE` (sqlx error code 2067 / message `UNIQUE constraint failed: worktrees.branch`) and return `AppError::Validation` ("a live worktree already records branch `<branch>`") instead of a 500. NEVER edit applied migrations 0001–0017.
- **Acceptance**: new repo test passes: creating two live worktrees with the same branch → second returns Validation; a worktree whose `outcome` is terminal (merged/rejected) frees its branch for reuse; `cargo nextest run --manifest-path lumina/Cargo.toml --profile quick -E 'package(lumina-core) & test(worktree)'` passes.

### Wave 2 — companion rewrite + server trigger (parallel, after wave 1)

### 3. Detached-integration ref-CAS choreography in the companion [L]
- **Files**: `lumina/companion/src/git/mod.rs`, `lumina/companion/src/git/shell.rs`, `lumina/companion/src/git/fake.rs`, `lumina/companion/src/executor.rs`, `lumina/companion/tests/executor.rs`
- **Depends on**: 1
- **Action**: Widen `GitBackend` with `resolve_branch_tip` / `attach_worktree_detached` / `detach_worktree` / `update_branch_ref`, shrink `merge()` to `(worktree, source, no_ff)`, and rewrite `Executor::merge_worktree` to the detached ref-CAS choreography; also resolve `CreateWorktree`'s committish `base` companion-side.
- **Detail**: Trait: `resolve_branch_tip(branch) -> Result<Sha, GitError>` (`git rev-parse --verify refs/heads/<b>^{commit}`; missing → NotFound); `attach_worktree_detached(path, committish) -> Result<Sha, GitError>` (`git worktree add --detach <path> <committish>`; an on-disk-but-unregistered dir → map to `GitError::Engine`, surfaced as `Failed{Internal}`); `detach_worktree(path, committish) -> Result<Sha, GitError>` (`git -C <path> checkout --detach <committish>`); `update_branch_ref(branch, new, expected_old, reflog_msg) -> Result<(), GitError>` (`git update-ref -m <msg> refs/heads/<b> <new> <old>`; classify stderr `is at … but expected` AND `unable to resolve`/deleted-ref as a NEW `GitError` CAS-lost variant — LC_ALL=C is already pinned in `base_command()`). Remove ShellGit's checked-out-branch guard from `merge()` (shell.rs ≈452–466). Executor choreography per `## Approach` steps 1–7 (abort-THEN-detach ordering explicit; executor-side `head_of == expected_old_tip` guard; AlreadyUpToDate skips the CAS; CAS-lost → `Failed{TargetMoved}` with no rollback; reachability rollback unchanged; `target_checkout` hint derived from the pre-fetched `worktree_states()` — the target is "checked out elsewhere" when some non-integration worktree record carries `branch == refs/heads/<target>`, `dirty` from its status). `CreateWorktree` arm: resolve `base` committish → `Sha` before `create_worktree`. Fake: four new FIFO queues + `FakeCall` variants; adjust `FakeCall::Merge` shape; keep the object-safety assertion. Rewrite executor unit tests: happy detached merge (call-sequence assert incl. CAS), legacy on-branch migration (attach exists → detach called after abort), CAS-lost → `TargetMoved`, AlreadyUpToDate skips CAS, reachability rollback precedes any ref move, hint derivation. Check the `Reconcile` arm still composes with detached state (it reads `worktree_states` filtered to the managed root — `branch: None` records must not panic).
- **Acceptance**: `cargo clippy --workspace --manifest-path lumina/Cargo.toml --all-targets` clean; `cargo nextest run --manifest-path lumina/Cargo.toml --profile quick -E 'package(lumina-companion) & binary(executor)'` passes.

### 4. `execute_worktree_create` server trigger + merge-hint surfacing [L]
- **Files**: `lumina/server/src/mcp/worktrees.rs`, `lumina/server/src/http/worktrees.rs`, `lumina/server/src/mcp/mod.rs`
- **Depends on**: 1, 2
- **Action**: Add the `execute_worktree_create` MCP tool + `POST /api/sprints/{sprint_id}/worktree/execute` HTTP mirror (count gate 85→86), and surface `Outcome::Merged.target_checkout` in the execute-merge responses.
- **Detail**: `execute_worktree_create_flow(sprint_id, branch, base_ref)` mirrors `execute_worktree_merge_flow`'s shape: pre-flights (sprint exists + non-terminal status; sprint does not already own a worktree — pre-check for a clean 422 rather than a constraint 500; companion connected + split-brain repo-root guard, reusing the merge flow's helpers; `branch` non-empty) → NO merge lease (sequential executor, no ref mutation) → dispatch `Intent::CreateWorktree { branch, base: base_ref }` with no DB tx held → on `Outcome::WorktreeCreated { path, branch, head }` record via existing `repo::create_worktree` (`NewWorktree { owning_sprint_id, path: <ground-truth path>, branch, base_ref }`; a migration-0018 Validation propagates as 422) → return `{ worktree_id, path, head }`. `Failed`/transport → MCP `internal_error` / HTTP 502 `{"error":{"kind":"companion",…}}`, mirroring the merge flow. Merge-hint surfacing: when the recorded merge outcome carries `target_checkout`, include it in the MCP/HTTP success payload plus a human hint string ("target branch was checked out at `<path>`<, with uncommitted changes>; refresh it with `git reset --keep <merge_sha>`"). Update the `mcp/mod.rs` count-invariant test 85→86 (+ membership list, uniqueness, and any stale count comments).
- **Acceptance**: `cargo clippy --workspace --manifest-path lumina/Cargo.toml --all-targets` clean; `cargo nextest run --manifest-path lumina/Cargo.toml --profile quick -E 'package(lumina-server) & test(mcp)'` passes (count gate at 86).

### Wave 3 — integration tests (parallel, after wave 2)

### 5. ShellGit integration tests for the new methods [M]
- **Files**: `lumina/companion/tests/shell_git.rs`
- **Depends on**: 3
- **Action**: Add real-temp-repo tests for `resolve_branch_tip`, `attach_worktree_detached`, `detach_worktree`, and `update_branch_ref`.
- **Detail**: Reuse `TestRepo` fixtures. Cover: `resolve_branch_tip` happy + missing-branch → NotFound; `attach_worktree_detached` yields a worktree that `worktree_states()` reports with `branch: None` while the same branch is checked out in the primary (no refusal); `detach_worktree` migrates a worktree that currently has a branch checked out (branch ref must NOT move) and re-points an already-detached one; `update_branch_ref` happy CAS (+ reflog message recorded via `git reflog`), CAS-lost after an out-of-band `git commit` on the target → CAS-lost error variant, deleted-ref → CAS-lost variant; merge-in-detached-HEAD end-to-end at the backend level (detach → merge → ref advance → primary checkout of target untouched on disk).
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml --profile quick -E 'package(lumina-companion) & binary(shell_git)'` passes.

### 6. Cross-plane e2e: regression + create scenario [M]
- **Files**: `lumina/server/tests/companion_e2e.rs`
- **Depends on**: 3, 4
- **Action**: Drop the `detach_primary()` workaround (it becomes the regression assertion) and add an `execute_worktree_create` e2e scenario.
- **Detail**: Merge scenario: REMOVE the `repo.detach_primary()` call — the primary checkout stays ON `main` and the merge must succeed (the brief's regression test); assert the response carries the `target_checkout` hint, `git rev-parse main` agrees with `merge_sha`, the §H reachability asserts hold, and the primary checkout's working tree files on disk are untouched. Conflict scenario: likewise keep the primary on-branch; semantics unchanged (no DB write, lease released). Delete the now-dead `detach_primary` helper + the ≈28–31 workaround comment. New create scenario: seed sprint (draft/ready), call the HTTP mirror with `{branch: "sprint/<x>", base_ref: "main"}` → assert 200, on-disk worktree at the sanitised path with detachable HEAD at main's tip, DB row recorded with the ground-truth path, owning sprint linked; then a second create for a DIFFERENT sprint with the SAME branch → 422 (migration-0018 index).
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml --profile quick -E 'package(lumina-server) & binary(companion_e2e)'` passes (requires `git` on PATH).

### Wave 4 — adoption pass (parallel, after wave 3)

### 7. Runbook + CLAUDE.md + ADR adoption [M]
- **Files**: `lumina/docs/runbooks/dogfood-lifecycle.md`, `lumina/CLAUDE.md`, `CLAUDE.md`, `docs/adr/0006-git-execution-companion.md`
- **Depends on**: 6
- **Action**: Rewrite the merge/worktree procedures around the companion execution plane and update every stale count/constraint statement.
- **Detail**: Runbook §E: prefer `execute_worktree_create { sprint_id, branch, base_ref }` (companion creates AND records); keep manual `git worktree add` + record-only `create_worktree` as the no-companion fallback. §H: prefer `execute_worktree_merge` over manual merge + `record_worktree_merge` (which remains the fallback/audit verb); document the conflict outcome (no DB write — surface as open question/finding), `AlreadyUpToDate` idempotent re-runs, the lease "already in flight" rejection (does NOT queue — retry same-target merges in dependency order), and the stale-primary remedy (`git reset --keep <merge_sha>`, prompted by the response hint). Encode the invariants: a worktree merges EXACTLY ONCE (owner in `review` pre-flight; `done` after); follow-up work after a merge = NEW sprint + NEW worktree off the updated base; the old "never check out integration/target branches" rule is RELAXED for merges (note why). `lumina/CLAUDE.md`: tool-surface counts 85→86 (incl. the count-invariant sentence and § Companion execution route list — add the new POST route); REWRITE the Step-1b "Operator constraint" paragraph (BranchInUse for merges is gone; describe detached integration + ref-CAS + `TargetMoved` + the hint field). Root `CLAUDE.md`: update both 85-count mentions and the Step-1b sentence in the lumina section. ADR-0006: append a short dated amendment section recording the detached-integration + ref-CAS revision (supersedes the checked-out-target constraint; §H gate unchanged; CAS failure = new coarse outcome).
- **Acceptance**: `rg -n '\b85\b' CLAUDE.md lumina/CLAUDE.md` shows no stale tool-count claims; `rg -in 'BranchInUse' lumina/CLAUDE.md` matches only the rewritten (create-path/historical) wording; runbook §H no longer instructs a manual `git merge` as the primary path.

### 8. Plugin skills adoption [M]
- **Files**: `claude/plugins/lumina-story-blocks/skills/run-sprint/SKILL.md`, `claude/plugins/lumina-story-blocks/skills/compose-sprint/SKILL.md`, `claude/plugins/lumina-story-blocks/skills/lifecycle/SKILL.md`, `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`, `claude/plugins/lumina-story-blocks/CONVENTIONS.md`
- **Depends on**: 6
- **Action**: Teach the lifecycle skills to drive `execute_worktree_merge` / `execute_worktree_create` and update the MCP catalogue.
- **Detail**: `run-sprint` Step 6: replace the manual-merge instruction with `set_sprint_status review` → `execute_worktree_merge { worktree_id }`; on `conflicted` → record a finding / open question and stop (companion already restored the worktree); on "already in flight" → retry same-target merges in dependency order; on `TargetMoved` → re-run (next attempt resolves the new tip); surface the stale-primary hint to the operator verbatim. `compose-sprint`: mint the sprint worktree via `execute_worktree_create` (fallback: manual git + record-only `create_worktree` when no companion is connected); note the unique-live-branch constraint (branch names must not collide with a live worktree, nor post-sanitisation per the brief). `lifecycle` advisor: advise the execute path at the merge step. `mcp/SKILL.md` catalogue: add `execute_worktree_create` (params, outcomes, 422 cases), update `execute_worktree_merge` (new `target_checkout` hint field, `TargetMoved` failure, BranchInUse-for-merges removed), bump any stated tool count to 86. `CONVENTIONS.md`: touch ONLY if it states a tool count or merge guidance (check; otherwise leave).
- **Acceptance**: `rg -n 'record_worktree_merge' claude/plugins/lumina-story-blocks/skills/run-sprint/SKILL.md` matches only fallback/audit wording (not the primary instruction); `rg -n 'execute_worktree_create' claude/plugins/lumina-story-blocks/skills/{compose-sprint,mcp}/SKILL.md` matches in both.

## Dependency Graph

Batch 1 (parallel): Tasks 1, 2
Batch 2 (parallel, after batch 1): Tasks 3, 4
Batch 3 (parallel, after batch 2): Tasks 5, 6
Batch 4 (parallel, after batch 3): Tasks 7, 8

Wave boundaries are also the GitBackend trait-gap checkpoints (E11 precedent: trait gaps surface to the orchestrator and are fixed BETWEEN waves, never mid-wave).

## Verification

Orchestrator-owned, two tiers (sub-agents run only clippy + their task's narrow nextest filter):

1. **Per-wave checkpoint** (before each batch commit): `cargo build --workspace --manifest-path lumina/Cargo.toml` + `cargo nextest run --manifest-path lumina/Cargo.toml --profile ci` — every commit non-broken and bisectable.
2. **Final full pass**: build + `--profile ci` tests + `cargo clippy --workspace --manifest-path lumina/Cargo.toml --all-targets` + the two dependency smoke gates (`lumina-server` must show no `git2|gix`; `lumina-companion` must show no `lumina-(core|server)|sqlx|axum`) + `cargo audit --file lumina/Cargo.lock` (no new deps expected, advisory check only).
3. **Manual smoke** (optional): `cargo run --manifest-path lumina/Cargo.toml -p lumina-server --bin lumina -- --with-companion` after a workspace build; drive one execute-create + execute-merge against a scratch repo while the primary checkout sits on the target branch.

## Risks

- **Legacy on-branch integration worktrees** (created by Step-1b code) would have their branch ref moved by a naive `reset --hard` — mitigated by using `checkout --detach` for re-pointing (Task 3) and a dedicated migration test (Tasks 3, 5).
- **Ref deleted mid-flight** surfaces differently from a moved ref in update-ref stderr — both classified as `TargetMoved` (Task 3); the re-run path then reports `NotFound` from `resolve_branch_tip`.
- **Migration 0018 aborts on a dirty dev DB** holding duplicate live branches — fail-loud by design; dev DB is gitignored/recreatable; remedy documented in the migration header. NEVER edit applied migrations 0001–0017.
- **Wire changes without a version bump** — accepted E12 precedent (wire unshipped); `#[serde(default)]` on the new `Merged` field keeps old frames deserialising.
- **Stale primary checkout after a CAS advance** shows spurious "undo-the-merge" diffs; if the operator commits there they revert the merge — mitigated by User Decision 4 (outcome hint incl. dirty flag + runbook `git reset --keep` remedy).
- **Scope ~21 unique files** exceeds the ~15 guideline — accepted in User Decision 1; contained by strictly sequential waves with ≤2 parallel agents and no file overlap within a wave.
- **Branch-name sanitisation collisions** (`feature/auth` → `integration-feature-auth`) pre-date this plan and are documented guidance, not code (Task 8 notes it in compose-sprint).
- **e2e depends on a real `git` on PATH** — already true of the existing suite; the `quick` nextest profile keeps these out of sub-agent inner loops.
