<!-- Generated from execution-record.toml. Do not edit by hand. -->

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E8 | widen-the-unshipped-protocol-wire-types | 2026-06-10 | `720e52e` | 1 file |
| E9 | migration-0018-partial-unique-live-branch-index | 2026-06-10 | `720e52e` | 2 files |
| E10 | detached-integration-ref-cas-choreography-in-the-companion | 2026-06-10 | `720e52e` | 6 files |
| E11 | execute-worktree-create-server-trigger-merge-hint-surfacing | 2026-06-10 | `720e52e` | 3 files |
| E12 | shellgit-integration-tests-for-the-new-methods | 2026-06-10 | `720e52e` | 1 file |
| E13 | cross-plane-e2e-regression-create-scenario | 2026-06-10 | `720e52e` | 1 file |
| E21 | runbook-claude-md-adr-adoption | 2026-06-10 | `4607b5d` | 4 files |
| E22 | plugin-skills-adoption | 2026-06-10 | `4607b5d` | 5 files |

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E2 | Migration 0018 created at lumina/core/migrations/ instead of the plan's lumina/migrations/ path | 2026-06-10 | | No lumina/migrations/ directory exists; migrations 0001-0017 live at lumina/core/migrations/ where core's sqlx::migrate!("./migrations") reads them. The root CLAUDE.md prose citing lumina/migrations/0004_repo_links.sql is stale on this path. File created at the canonical location. | — |
| E3 | Fifth GitBackend trait method resolve_committish added beyond the plan's four | 2026-06-10 | | resolve_commit is a private inherent helper on ShellGit and the trait's resolve is the conflict-resolution op; the executor consumes only Arc<dyn GitBackend>, so no existing trait surface could resolve a general committish (HEAD~2/tags/SHAs). Promoted as GitBackend::resolve_committish, ShellGit delegating to the private helper. Also: occupied-path attach maps to Failed{GitFailure} (not the plan's parenthetical Failed{Internal}) because the centralised kind_for table maps Engine->GitFailure; the explicit GitError::Engine instruction was honoured over the parenthetical. | — |
| E4 | Sprint pre-flight reads issued via lumina_core::db::scalar_opt instead of a repo::* read | 2026-06-10 | | No repo::* read exposes a sprint-by-id lookup (only set_sprint_status reads it, inside its write tx) and lumina/core was outside the task's file ownership. The flow issues three read-only scalar SELECTs through the public db::scalar_opt seam; the mcp/mod.rs no-new-SQL doc-invariant was reworded to scope to writes. Follow-up option: add repo::get_sprint_status in lumina/core/src/repo/runs_sprints.rs to restore the strict invariant. | — |
| E5 | Create-scenario assertions adjusted: created worktree is branch-attached (not detached) and the literal duplicate-branch sequence yields 502 BranchInUse (not the 0018 422) | 2026-06-10 | | ShellGit::create_worktree runs git worktree add -b <branch>, so the created worktree is ATTACHED to the new branch at the resolved base tip (only the merge-side integration worktree is detached); asserted HEAD commit == main tip == response head plus symbolic-ref == the branch. The companion's git runs before the record write, so a same-branch re-create fails in git first (502 BranchInUse); the 0018 422 path was covered separately via a record-only row holding the branch, then an execute-create hitting the partial UNIQUE index. Both mechanisms tested; a DB-side branch pre-flight in the flow would be needed to make the literal sequence a 422. | — |
| E24 | Round-1 /review findings R1-R28 applied to the landed ref-CAS pass via /review-apply (27 fixed incl. 2 partial, R12 left open pending schema decision) | 2026-06-10 | `69dd9a5` | Round-1 review surfaced git-argv injection hardening (-- separators + leading-dash reject), real --detach NotFound classification, prunable-worktree tolerance, canonicalise-aware path identity, structured failure_kind envelopes with 422/502 split, worktree-keyed merge lease, identity split-brain guard (resolves the concern noted in E4 by moving sprint reads behind repo::get_sprint), commit-sha shape validation, 0018 liveness-axis documentation, broken skill-doc links, and 13 coverage gaps; applied across three commits with full workspace verification green (build, 597 tests, clippy, audit) | — |
| E25 | Round-2 /review-apply closed the four remaining ledger items R12+R31-R33 (user-approved schema + tool-surface changes); review ledger now fully dispositioned (31 fixed, 2 verified-clean) | 2026-06-10 | `b1888fa` | User selected R12,R31-R33 for apply, authorising: migration 0019 (per-repo live-branch index via nullable worktrees.repo_link_id + deleted_at liveness alignment, closing R11's deferral), best-effort RemoveWorktree compensation on create record-failure, the remove_task_commit tool taking the surface to 87 (count-invariant test updated), and the remaining skill-link depth repairs; full workspace verification green (build, 605 tests, clippy) | — |

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-06-10 | 25 entries: status-transition × 2, deviation × 6, verification × 9, task-completion × 8 | 4607b5d, 55d4c29, 69dd9a5, 720e52e, 9490ee8, 96b6478, b1888fa |
