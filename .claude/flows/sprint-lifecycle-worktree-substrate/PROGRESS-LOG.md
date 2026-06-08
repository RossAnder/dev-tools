<!-- Generated from execution-record.toml. Do not edit by hand. -->

# Progress Log: sprint-lifecycle-worktree-substrate

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E5 | migration-0016-sprint-lifecycle-worktrees | 2026-06-08 | `108bbcc` | 2 files |
| E6 | domain-sprintstatus-worktree-types | 2026-06-08 | `108bbcc` | 3 files |
| E7 | thread-checkpoint-row-mapping | 2026-06-08 | `108bbcc` | 3 files |
| E13 | repo-worktrees-mutators | 2026-06-08 | `3f87d3a` | 2 files |
| E14 | repo-sprint-lifecycle-set-status | 2026-06-08 | `3f87d3a` | 1 files |
| E15 | repo-set-task-checkpoint | 2026-06-08 | `3f87d3a` | 1 files |
| E16 | repo-claim-guard-checkpoint-freeze | 2026-06-08 | `3f87d3a` | 1 files |
| E17 | repo-quiescence-status-freeze | 2026-06-08 | `3f87d3a` | 1 files |
| E18 | mcp-worktree-family | 2026-06-08 | `3f87d3a` | 2 files |
| E19 | mcp-set-sprint-status | 2026-06-08 | `3f87d3a` | 1 files |
| E20 | http-worktree-mirrors | 2026-06-08 | `3f87d3a` | 3 files |
| E21 | e2e-thread-extension | 2026-06-08 | `3f87d3a` | 1 files |
| E22 | lifecycle-guard-tests | 2026-06-08 | `3f87d3a` | 2 files |
| E29 | docs-lumina-claude-md | 2026-06-08 | `cdd1716` | 1 files |
| E30 | docs-context-adr-0005 | 2026-06-08 | `cdd1716` | 4 files |
| E31 | docs-root-claude-skill-md | 2026-06-08 | `cdd1716` | 2 files |

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E4 | Added worktree_id/predecessor_sprint_id:None to 10 existing NewSprint construction sites | 2026-06-08 | `108bbcc` | serde(default) gives deserialization defaults, not Rust struct-literal defaults; 10 sites across 8 files failed to compile | — |
| E10 | Activated sprint in http/execution.rs in-module claim tests for the stricter guard | 2026-06-08 | `3f87d3a` | The plan's test-repair list did not enumerate http/execution.rs's seed-then-claim tests | — |
| E11 | Aligned set_sprint_status SQL to the actual sprints schema (no updated_at/deleted_at) | 2026-06-08 | `3f87d3a` | sprints has only id/title/status/created_at + 0016 FK cols; runtime sqlx surfaced the bad columns only at test time | — |
| E12 | Fixed http/worktrees.rs to use crate::domain::TaskCommit not repo::TaskCommit | 2026-06-08 | `3f87d3a` | TaskCommit is a domain type not re-exported under repo::; T11's cargo check was blocked by a Bash fork flake | — |
| E28 | Corrected ADR cross-reference filenames in lumina/CLAUDE.md migration-0016 block | 2026-06-08 | `cdd1716` | T14 could not read docs/adr/ and guessed the wrong ADR filename for the layer-2 + runtime-freeze links | — |

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|---------------|------|--------|------------------|
| (none) | | | | | |

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-06-08 | 32 entries: status-transition × 2, verification × 9, deviation × 5, task-completion × 16 | `108bbcc`, `3f87d3a`, `cdd1716` |
