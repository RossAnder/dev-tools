---
name: optimise-apply
description: |
  This skill should be used when the user asks to implement optimisation findings
  from a prior /optimise run — reads a findings file at .claude/optimise-findings--
  <scope>.md (or findings visible in current conversation), filters by item numbers
  or severity, pre-analyses, clusters by file, launches parallel implementation
  sub-agents, verifies, and deletes the consumed findings file. Triggers on phrases
  like "apply the optimise findings", "implement items 1,3,5 from the findings",
  "apply critical perf items", "apply all from the optimise report". Requires an
  existing findings file or structured findings in context — if none, the skill
  should tell the user to run /optimise first.
argument-hint: "[item numbers, e.g. \"1,3,5\" or \"all\" or \"critical\"]"
disable-model-invocation: false
---

# Apply Optimisation Findings

Implement findings from the `optimise` skill. Findings live in
conversation context or a scope-keyed file at
`.claude/optimise-findings--<scope>.md`. Prefer context; fall back to
the file. If neither exists, tell the user to run `/optimise` first —
this skill applies findings, never generates them. Five steps: parse
and filter, pre-analyse in the main conversation, cluster by file,
launch parallel sub-agents, verify. Pre-digested reasoning passes to
agents so they execute rather than deliberate. Delete the consumed
findings file when done.

## Step 1: Parse Findings and Determine Scope

1. Locate the most recent `## Optimization Findings` report. Check in
   order: (a) conversation context, (b) files matching
   `.claude/optimise-findings--*.md`. If multiple exist, list them and
   ask which to apply. If `$ARGUMENTS` contains an explicit path (e.g.
   `.claude/optimise-findings--src-worker.md`), load it directly.
2. Interpret `$ARGUMENTS` to filter:
   - Item numbers (`1,3,5`) — apply only those items.
   - `all` — every item including suggestions.
   - `critical` — critical-severity items only.
   - `critical,warnings` — critical and warning items.
   - Empty — all critical and warning items, skip suggestions.
3. If `$ARGUMENTS` is explicit (numbers, `all`, `critical`, or a
   severity list), proceed without confirmation. Otherwise, list the
   selected findings and confirm the plan before proceeding.
4. Record the resolved selection so Step 3 can cluster it and the final
   summary reports implemented vs skipped accurately.

## Step 2: Pre-analyse Complex Findings (main conversation)

**Use extended thinking at maximum depth.** Sub-agents cannot think
deeply, so complex analysis happens here. Before delegating:

- Read every file referenced by the selected findings in full.
- For findings involving novel APIs, complex algorithmic changes, or
  cross-cutting patterns, reason through the approach now.
- Verify target files still match the findings — confirm code at the
  referenced lines has not drifted since `optimise` ran. Mark stale or
  inapplicable findings to be skipped.
- Resolve ambiguities in the `Recommended` section. If multiple
  approaches are viable, pick one and record the rationale.
- Include the pre-analysed reasoning in each agent's prompt so agents
  execute rather than deliberate.

## Step 3: Cluster Findings by File

Group selected findings by file or closely related cluster. One
implementation agent launches per cluster in Step 4, so cluster
boundaries determine parallelism. Files sharing findings or with
interdependent changes belong together.

**Cluster grouping is the primary conflict-avoidance strategy.** No two
agents ever edit the same file concurrently. If findings cannot be
separated into non-overlapping clusters (multiple findings hitting one
file from different angles), **sequence those agents rather than
parallelise them**. Only use `isolation: "worktree"` as a last resort
when overlapping edits are unavoidable — worktree merges are slow and
risky.

If findings have dependencies (adding an interface before consuming it,
changing a type that flows through multiple files), note them so agents
sequence correctly. **Concurrency changes need extra care.** A
sync-to-async type change must land before any finding modifying its
callers. A shared-primitive swap (e.g. Mutex to channel) must sit in the
same cluster as, or strictly before, findings touching its consumers.

## Step 4: Launch Implementation Agents

Launch agents in parallel via the Agent tool
(`subagent_type: "general-purpose"`), one per cluster, capped at 3–4
concurrent. **Read `references/optimise-apply-agent-prompt.md` for the
full agent contract** — exact files, pre-analysed reasoning,
Context7/WebSearch requirements, minimum-change rule, and
skip-and-report protocol for stale findings.

**IMPORTANT: emit all independent Agent tool calls in a single response
message** so they execute concurrently. Dependent (same-file) agents
run sequentially after; commit earlier batches first so later failures
are revertible.

## Step 5: Verification and Cleanup

After all implementation agents complete, launch a single **verification
sub-agent** to keep verbose build/test output out of the main context.
**Read `references/verification-agent-prompt.md` for the verification
contract** — including concurrency-specific checks (async-aware locks,
bounded task spawning, channel capacity rationale, cancellation safety)
for findings that modify concurrency primitives.

If verification fails, **use extended thinking at maximum depth** to
diagnose, then fix directly or launch a targeted fix agent and re-run.

**Use extended thinking for the final summary.** Cross-reference agent
results and present:

```
## Applied Optimizations

### Implemented
- [#N] [file:line] Summary — (severity)

### Skipped
- [#N] [file:line] Reason

### Verification
- Build: pass/fail
- Tests: pass/fail/none
```

**Clean up** — delete the consumed `.claude/optimise-findings--*.md`
file, if one existed.

## Important Constraints

- **No extended thinking in sub-agents** — complex reasoning happens in
  Step 2. Give agents pre-digested analysis, not open problems.
- **Do not apply suggestions** unless `$ARGUMENTS` explicitly includes
  them (via `all` or by number).
- **Do not introduce new dependencies** without flagging it first.
- **Do not change public API contracts** unless the finding calls for
  it and the user confirmed.
- **Preserve behaviour** — same observable result. If unsure, skip it.
- **One concern per edit** — no combined refactors or style fixes.
