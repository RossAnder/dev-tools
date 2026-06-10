<!-- Generated from execution-record.toml. Do not edit by hand. -->

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E4 | populate-lumina-protocol-wire-types | 2026-06-10 | `2433bcc` | 3 files |
| E5 | gitbackend-trait-neutral-types-fakegitbackend-companion-lib-scaffolding | 2026-06-10 | `2433bcc` | 6 files |
| E8 | shellgit-backend-quarantined-run-git-real-repo-tests | 2026-06-10 | `c4dec13` | 4 files |
| E9 | companion-executor-intent-backend-outcome | 2026-06-10 | `c4dec13` | 3 files |
| E10 | server-companion-seam-registry-lease-ws-endpoint | 2026-06-10 | `c4dec13` | 7 files |
| E15 | companion-connection-loop-binary | 2026-06-10 | `37cbbf9` | 3 files |
| E16 | execute-worktree-merge-mcp-tool-http-mirror-reachability-query | 2026-06-10 | `37cbbf9` | 5 files |
| E19 | serve-with-companion-co-launch | 2026-06-10 | `f06ca26` | 1 file |
| E20 | cross-plane-e2e-test | 2026-06-10 | `f06ca26` | 3 files |
| E29 | documentation-updates | 2026-06-10 | | 3 files |

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E11 | GitBackend widened with attach_worktree between waves: the frozen create_worktree was new-branch-only, but User Decision 1 requires attaching a worktree to the EXISTING integration target branch. | 2026-06-10 | `c4dec13` | create_worktree is documented and implemented as new-branch-only (git worktree add -b); attaching the existing target branch is a distinct git operation. Fixed via the plan's sanctioned between-waves protocol: one additive object-safe method (attach_worktree) implemented in ShellGit + FakeGitBackend, executor lazy-create arm switched to it, base-resolution workaround deleted. | — |
| E12 | CommitCheckpoint.stage_paths removed from the v1 wire: selective staging was inexpressible over the GitBackend trait (commit_all only) and has no Step-1b consumer. | 2026-06-10 | `c4dec13` | The trait exposes only commit_all; widening it for a feature with zero 1b consumers violates minimum-change. v1 is unshipped so the field was dropped without a version bump; checkpoint commits are commit-all semantics, selective staging reintroducible in a future protocol version. | — |
| E21 | Co-launch run invocation corrected in docs: bare cargo run -p lumina-server is ambiguous (two [[bin]] targets, no default-run), so docs use the --bin lumina form. | 2026-06-10 | | lumina-server ships two bins (lumina + pty_stub) with no default-run, so the suggested form errors; docs were written with cargo run --manifest-path lumina/Cargo.toml -p lumina-server --bin lumina -- --with-companion, matching the repo's existing disambiguation guidance. | — |

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-06-10 | 30 entries: status-transition × 2, verification × 15, task-completion × 10, deviation × 3 | 2433bcc, 37cbbf9, c4dec13, f06ca26 |
