# Idea: Sprint Manager + Worktree Merge Queue for Lumina PTY Multi-Session Work

> Captured: 2026-05-27
> Status: Idea — not yet planned

## Context

The lumina PTY service is being designed to support multiple concurrent agent sessions operating against the same codebase. The "unrelated edits detected" failure mode (where the PTY supervisor sees a working tree mutated by an off-channel writer) becomes a routine occurrence once two sessions share one checkout, so per-session isolation is needed.

A prior attempt at parallel agents creating their own worktrees ad-hoc (per-agent granularity, distributed merge logic) failed — too much manual merge work, increased risk of lost work. That experience pushed toward "no worktrees, sequence work instead."

This idea revisits worktrees at a coarser granularity (per-session, not per-agent) with a central coordinator that owns merge orchestration, taking advantage of lumina's task graph + `files_touched` data which didn't exist in the prior attempt.

## Core Proposal

A **sprint manager** that:

1. Spawns each session into its own git worktree.
2. Provides cross-session context (so any session can see what other sessions are working on, without inspecting their working trees directly).
3. Constrains parallel agents within a session to share that session's worktree with safe concurrent access.
4. Owns merge orchestration via a **worktree merge queue** that gates every merge into main behind a review step — a "PR gate for worktrees."

## Worktree Merge Queue Design

Completed sessions push their branch onto a FIFO queue. Each queue entry is routed at enqueue time into one of three lanes based on cheap signals:

| Condition | Lane | Cost |
|---|---|---|
| Main hasn't moved since branch-base | Fast-forward | None |
| Main moved, no overlap of `files_touched` with intervening merges | Verification-only re-run | Cheap |
| Main moved AND `files_touched` overlaps with intervening merges | Full conflict + semantic review | Expensive |

The full-review lane should be the minority by design.

### Load-bearing details

- **Stale-while-queued handling**: each enqueued item carries the branch-base it was reviewed against. If main moves while the item is waiting (because an earlier queue entry merged), the item is re-routed back through lane selection — never silently merged against a base it wasn't reviewed against. Mirrors GitHub's merge queue behaviour on intervening merges.
- **Post-merge verification gate**: after each merge into main, run the project's verification commands (from the plan's `## Verification Commands` block, or `/test-bootstrap`'s marker block). On failure: revert the merge, mark the session's branch needs-fix, surface it back to the session. Catches residual semantic-conflict cases that the pre-merge reviewer missed.
- **N-way ordering dissolves into FIFO**: the queue's serialisation means we don't need to reason about three sessions simultaneously merging the same file — each merge updates main and forces the next entry to re-evaluate against the new state.

## Why Per-Session Worktrees Are Different This Time

The prior failed attempt had:
- Per-agent (not per-session) granularity → N×M concurrent worktrees
- Merge logic distributed across agents → each agent inventing its own strategy
- No task graph / no `files_touched` data → impossible to predict overlap

The new model has:
- Per-session granularity → N concurrent worktrees, collapsing the fan-out
- Central manager owns merge orchestration → one strategy, one place
- Lumina's task spec carries `files_touched` (incl. multi-repo via migration 0004) → gate can route on real data
- Lumina's task `attributes` + `activity` log → reviewer agent has intent context, not just diffs

## Merge-Conflict Strategy

Discussion explored three failure modes git's 3-way merge can produce on a file touched by two sessions:

1. **Non-overlapping line ranges** → git merges clean. Majority case. No manager involvement.
2. **Overlapping line ranges** → conflict markers. Resolver agent or human surface required.
3. **Semantic conflict, no textual overlap** → git merges clean, produces broken result. The silent failure mode — no marker, no warning. Examples: A renames a section, B adds a cross-reference to the old name; A removes a build command from a list, B adds prose that depends on it existing.

The pre-merge reviewer in the full-review lane addresses (3) — the silent class — by reading the diff against current main, not just the original branch base, with intent context from lumina.

**Design decision** (user preference, captured 2026-05-27): do NOT restrict overlapping `files_touched` at dispatch time. Allow concurrent work on the same file; force a judgement call at merge time instead. Rationale: pre-dispatch serialisation only addresses textual collision; the merge-time reviewer additionally catches the semantic-conflict class, and avoids serialising work that would have composed fine.

### Reviewer agent inputs

- The session's diff
- Current main
- The merge-base
- Lumina's task intent: `attributes`, `activity` log, `files_touched`

### Failure path

Prefer **rebase-and-retry** over discard. If the gate says "this no longer composes with main," the session's branch rebases onto current main, re-runs its verification commands, and re-enters the queue. Discarding completed work is a last resort with a human in the loop.

## Mid-Session Merge-Back (Out of Scope for v1)

The original idea included merging non-breaking partial commits back to main mid-session. This re-introduces the original confusion in a new form — if a session's base moves under it mid-flight, the session needs to know.

**Recommendation**: start with **merge-only-on-session-complete**, long-lived worktrees rebased forward at dispatch time. Add mid-session merge-back only once the simpler model is stable.

## Integration With Existing Lumina Primitives

- **The queue itself is a natural lumina work-item kind** (or a side table with events flowing through the existing outbox) → gets git-export audit and SPA visibility for free.
- **Sprint manager becomes "dispatches sessions + owns the queue"** — one component, not two separate responsibilities.
- **`files_touched` widening from migration 0004** (per-entry `{repo, path}` for multi-repo projects) is the right shape for the queue's lane-routing logic; no schema change needed for the basic case.
- **The PTY supervisor's "unrelated edits detected" signal** becomes the diagnostic for a violated isolation invariant — if it fires inside a session's worktree, something has leaked across worktrees.

## Open Questions for the Future Plan

- **Queue priority**: FIFO by completion time is the obvious default, but lumina's task graph could justify priority bumps (e.g. blocking dependency on a downstream task). Worth deciding up-front or leave as v2?
- **Backpressure**: if review is slow and the queue backs up, dwell time itself becomes a cost (stale-while-queued forces re-review). What's the queue-depth alarm threshold?
- **Worktree lifecycle**: when does a session's worktree get cleaned up? Probably "after successful merge + grace period for inspection," with failed merges keeping the worktree alive for debugging. Needs explicit policy.
- **Reviewer agent shape**: is this a new dedicated agent type, or does it compose existing `/review` / `/review-apply` skills? The lumina task-intent integration suggests a new dedicated kind.
- **Cross-session context API**: what does "the manager provides cross-session context" actually look like at the MCP / HTTP surface? Probably a read tool that lists active sessions + their `files_touched` + their stated intent.

## Related Code

- `lumina/src/pty/supervisor.rs` — PTY supervisor that detects unrelated edits
- `lumina/src/mcp.rs` — MCP tool surface; queue + session would extend this
- `lumina/migrations/0004_repo_links.sql` — multi-repo `files_touched` already supports `{repo, path}` form
- `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md` — agent-facing usage guide; needs sprint-manager + queue tools once designed
