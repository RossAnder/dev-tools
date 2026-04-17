---
description: Implement a plan or task using parallel sub-agents with research, progress tracking, and verification
argument-hint: [plan path or task description]
---

## Flow Context

Every invocation of this command reads (and may write) a per-flow `context.toml` under `.claude/flows/<slug>/`. The schema and rules below are the single source of truth — follow them verbatim.

### Canonical Flow Schema

```toml
slug = "auth-overhaul"
plan_path = "docs/plans/auth-overhaul.md"
status = "in-progress"
created = 2026-04-08
updated = 2026-04-16
branch = "auth-overhaul"

scope = ["src/auth/**", "src/middleware/auth.rs"]

[tasks]
total = 10
completed = 3
in_progress = 1

[artifacts]
review_ledger = ".claude/flows/auth-overhaul/review-ledger.toml"
optimise_findings = ".claude/flows/auth-overhaul/optimise-findings.toml"
```

### Status vocabulary

`status` takes one of four string values: `draft`, `in-progress`, `review`, `complete`.

- `draft` — written by `plan-new` at creation.
- `in-progress` — written by `implement` when it starts a task; written by `plan-update` after work resumes.
- `review` — written only by `plan-update` when a plan enters a review phase between implementation rounds.
- `complete` — written only by `plan-update` when all tasks are done or all remainders are deferred.

**Unknown-value rule**: if a command reads a `status` it doesn't recognise, it MUST treat it as `in-progress` (fail-soft) and proceed. Do not error.

### Field responsibilities

- `slug` — immutable after creation. Only `plan-new` writes it.
- `plan_path` — immutable after creation. For multi-file plans, `plan_path` points at the **outline file** (e.g. `docs/plans/auth-overhaul/00-outline.md`), not the directory.
- `created` — immutable after creation. **Every command that rewrites `context.toml` MUST preserve `created` verbatim.** Never regenerate it.
- `updated` — writeable by `plan-new`, `implement`, `plan-update`. Set to today's date (ISO 8601) on every write.
- `branch` — optional. `plan-new` sets it from `git branch --show-current` if that produces a non-empty string; otherwise the field is **omitted entirely** (not written as empty string). No other command writes `branch`. Resolution step 3 skips flows whose `branch` key is absent.
- `scope` — writeable by `plan-new` (initial derivation from the plan's "Affected areas" section, globs like `<dir>/**`) and by `plan-update reconcile` (may refine based on actual edits). Never empty after initial creation — if `plan-new` cannot derive anything, it writes the plan's affected directories as `<dir>/**` patterns.
- `[tasks]` — writeable by `plan-update` (all ops that touch progress); writeable by `implement` (`in_progress` counter only when starting/finishing).
- `[artifacts]` — **canonical, always written.** Paths are computed from `slug` but must be persisted in the TOML for stability. If `[artifacts]` is absent when read, commands compute from `slug` but MUST write it back on their next TOML write.

### Slug derivation

Slug = plan filename minus `.md` extension. Examples:
- `docs/plans/auth-overhaul.md` → slug `auth-overhaul`
- `docs/plans/auth-overhaul/00-outline.md` (multi-file) → slug `auth-overhaul` (parent directory name)

No additional slugification — the filename is already the slug.

### Flow resolution order (every command, every invocation)

1. **Explicit `--flow <slug>` argument**. If provided, use it verbatim. If `.claude/flows/<slug>/` doesn't exist, error.
2. **Scope glob match on the path argument**. For each `.claude/flows/*/context.toml` where `status != "complete"`, read the `scope` array. For each pattern, invoke the `Glob` tool with the pattern and check whether the target path appears in the result. If exactly one flow matches, use it. Skip `status == "complete"` flows entirely.
3. **Git branch match**. Run `git branch --show-current`. If the output is non-empty, look for a flow whose `context.branch` equals it (exact match, case-sensitive). Skip this step if output is empty (detached HEAD).
4. **`.claude/active-flow` fallback**. Read the single-line slug. If `.claude/flows/<slug>/` exists with a valid `context.toml`, use it. If the pointed-at directory is missing or the TOML is malformed, proceed to step 5.
5. **Ambiguous / none found**: list candidate flows (all non-complete flows with summary: slug, plan_path, status), ask the user.

### TOML read/write contract

- **Reading**: if `context.toml` is missing required fields (`slug`, `plan_path`, `status`, `created`, `updated`, `scope`, `[tasks]`, `[artifacts]`), prompt the user with the specific missing fields and the plan's current path. Do not synthesise defaults silently.
- **Reading**: if `context.toml` is syntactically invalid (can't be parsed as TOML), report the parse error and ask the user to fix manually. Do not attempt auto-repair.
- **Writing (preferred)**: use `tomlctl` (see skill `tomlctl`) — `tomlctl set <file> <key-path> <value>` for a scalar, `tomlctl set-json <file> <key-path> --json <value>` for arrays or sub-tables. `tomlctl` preserves `created` verbatim, preserves key order, holds an exclusive sidecar `.lock`, and writes atomically via tempfile + rename. One tool call per field — no Read/Edit choreography required.
- **Writing (fallback)**: if `tomlctl` is unavailable, Read the file, modify only the target line(s) via `Edit`, Write back. Preserve `created` verbatim. Preserve key order. Do not introduce inline comments.

### Flow-less fallback

When `/review` or `/optimise` run on code outside any flow (resolution ends at step 5 and user picks "no flow"):
- `/review` → `.claude/reviews/<scope>.toml`
- `/optimise` → `.claude/optimise-findings/<scope>.toml`

Slug derivation for flow-less scope: lowercase, replace `/\` with `-`, collapse `--`, strip leading `-` (preserved from pre-redesign).

### Completed-flow handling

Flows with `status = "complete"` are skipped by resolution step 2 (scope glob match). They remain on disk for audit but do not participate in auto-resolution. Users can still target them via explicit `--flow <slug>`.

# Implementation

Implement a plan, feature, or task by delegating work to parallel sub-agents. Handles work decomposition, research for novel steps, efficient parallelisation, progress reporting via Task tools, and verification.

Works with:
- **Plan files** — `/implement docs/plans/todo/prod_preparation/01-security-hardening.md`
- **Plan directories** — `/implement docs/plans/todo/prod_preparation/`
- **Specific items** — `/implement items 3,4,5 from docs/plans/todo/prod_preparation/00-outline.md`
- **Inline tasks** — `/implement add account lockout with progressive delays`
- **No arguments** — `/implement` auto-resolves the active flow via the 5-step flow resolution order (see Flow Context above): explicit `--flow <slug>`, scope glob match, git branch match, `.claude/active-flow` pointer, or user prompt

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning, tool usage, and deviation detection.

## Phase 1: Analyse and Decompose (main conversation — thinking enabled)

**Reason thoroughly through analysis and decomposition.** Front-load analysis here — the orchestrator has the broadest view, pre-digested instructions let agents execute rather than re-deliberate, and complex reasoning is verified once rather than N times. Research novel patterns, resolve ambiguities, and produce precise agent instructions.

1. **Load the work**:
   - **Resolve the flow** using the 5-step order documented in the Flow Context section above:
     1. Explicit `--flow <slug>` argument wins. If provided, use it verbatim; error if `.claude/flows/<slug>/` is missing.
     2. Scope glob match on the path argument — for each non-complete `.claude/flows/*/context.toml`, test every `scope` pattern via the `Glob` tool; use the flow if exactly one matches.
     3. Git branch match — `git branch --show-current`; pick the flow whose `context.branch` equals the output (skip on empty / detached HEAD).
     4. `.claude/active-flow` fallback — read the single-line slug; use it if `.claude/flows/<slug>/context.toml` exists and parses; otherwise fall through.
     5. Ambiguous / none found — list candidate non-complete flows (slug, plan_path, status) and ask the user.
   - Once a flow resolves, read its `context.toml` and extract `plan_path`. Read that plan file.
   - If $ARGUMENTS points to a plan directory, start with the **outline/master document** (e.g. `00-outline.md`) to understand scope, items, dependencies, and file targets. Then read only the detail documents relevant to the items being implemented — not every file in the directory.
   - If $ARGUMENTS points to a single plan file, read that file. If a flow also resolved, prefer the explicit plan-file argument but retain the flow context for Phase 4.5 writes.
   - If $ARGUMENTS is an inline task description, explore the codebase to understand the current state and determine what files need changing.
   - If $ARGUMENTS references specific items (e.g. "items 3,4,5"), extract only those from the plan.
   - **Track the flow context**: Note the resolved plan file path and flow `slug` — you'll need them for the Phase 4 report, Phase 4.5 sync, and `/plan-update` suggestions. If a flow resolved, update its `context.toml` now: set `status = "in-progress"`, set `updated` to today's ISO 8601 date, and increment `[tasks].in_progress`. **Preserve `created` verbatim** and preserve key order per the TOML read/write contract.
   - **Extract verification commands**: If the plan contains a `## Verification Commands` section, extract the build, test, and lint commands. These will be passed directly to the verification agent in Phase 3 — do not rely on the verification agent to re-discover them.
   - **Read source files selectively** — once scope is determined, read only files needed to resolve ambiguities or make decomposition decisions. Agents will read their own target files in full, so do not pre-read every file that will be modified.

2. **Research novel or complex steps**:
   - For any step involving unfamiliar APIs, recent framework features, or technically complex patterns, research NOW in the main conversation using Context7 and WebSearch. Resolving research here once is cheaper than having every agent re-investigate and lets you verify conclusions before delegating.
   - Resolve ambiguities in the plan — if a task could be implemented multiple ways, decide the approach here and document it in the agent instructions.

3. **Decompose into agent tasks**:
   - Break the work into discrete tasks, each owning specific files with no overlap.
   - Classify each task's complexity:
     - **Straightforward** — direct edits, well-understood patterns, clear examples in codebase
     - **Complex** — requires careful reasoning, multiple interacting changes, or novel API usage
   - For complex tasks, include the research findings and reasoning from this phase directly in the agent's prompt.
   - Identify dependencies between tasks. Tasks with no dependencies on each other can run in parallel.
   - **Target 3-4 parallel agents maximum** for implementation. More creates diminishing returns.

4. **Create Task tracking**:
   - Use TaskCreate for each task with a clear `subject` and `description`.
   - Set `addBlockedBy` for tasks that depend on others.
   - This provides visual progress in the UI and makes the work resumable if interrupted.

## Phase 2: Execute (parallel sub-agents)

Launch implementation agents grouped into batches by dependency order. Each batch runs in parallel; batches run sequentially.

**IMPORTANT: You MUST make all independent Agent tool calls within a batch in a single response message.** Do not launch them one at a time. **Do NOT reduce the agent count** — launch the full complement of agents for each batch. Each agent owns a distinct file cluster with no overlap.

### Agent dispatch rules

Every implementation agent prompt MUST include:
- The exact files to read and modify (absolute paths)
- **File read instructions**: "Read every file listed in your Files section in full before making changes. Also read any file you import from or export to, so you understand the integration surface."
- What the code should do after the change and why it's changing
- For complex tasks: the research findings and reasoning from Phase 1
- Specific API signatures or patterns to use (from Context7 research done in Phase 1)
- Clear success criteria — what "done" looks like
- Instruction: "You MUST use Context7 MCP tools to verify any new API usage before writing code — do not rely on training data alone"
- Instruction: "You MUST use WebSearch if uncertain about implementation details"
- Instruction: "Reason through each change step by step before editing"
- **Plan deviation protocol**: "If you discover that the plan's assumptions are wrong — a file doesn't exist, an API has changed, an interface differs from what the plan describes — do NOT silently improvise. Complete whatever changes you can that are unaffected, then report the deviation clearly in your output: what the plan assumed, what you found, and what was left undone. The orchestrator will decide whether to adapt or abort."

### Agent tool guidance

Include this tool guidance in each agent's prompt, tailored to its task:

- **Context7**: "You MUST use mcp__context7__resolve-library-id then mcp__context7__query-docs to verify API signatures, method parameters, and correct usage patterns before writing any code that uses framework or library APIs."
- **WebSearch**: "You MUST use WebSearch if you encounter an unfamiliar pattern, need to check for deprecations, or are unsure about the correct approach for the framework version in use."
- **Codebase exploration**: "Read related files to understand existing patterns before writing new code. Match the style, naming, and structure of surrounding code."
- **Diagnostics**: "LSP diagnostics are reliable when you first open a file and useful for understanding existing issues. However, after making edits, new diagnostics may be stale — do not automatically act on post-edit diagnostics. If new diagnostics appear after your edits, re-read the flagged lines to verify the issue is real before attempting a fix. For definitive verification, run a targeted build command (e.g. `cargo check -p crate_name`, `dotnet build path/to/Project.csproj`, `tsc --noEmit`) rather than relying on LSP. Leave full build and test runs to the verification agent."

### Batch execution

**Prompt-cache tip**: When launching the batch's agents, place shared context — file list, plan excerpts, verification commands, cross-cutting constraints — as a literal-equal preamble at the top of each agent prompt, with per-agent divergence (specific files, task details) below a clear divider. The 5-minute TTL prompt cache reuses the shared prefix across agents, reducing latency and cost. Keep the shared text byte-identical — whitespace differences defeat the cache.

For each batch:
1. Update all batch tasks to `in_progress` via TaskUpdate.
2. Launch all agents in the batch in a single response.
3. When agents return, check for **plan deviations** (see protocol above). If an agent reports a deviation:
   - Reason through the impact.
   - If the deviation is minor and the fix is clear, launch a targeted fix agent.
   - If the deviation is significant (wrong interface, missing file, architectural mismatch), pause execution, report the deviation to the user, and suggest running `/plan-update deviation` before continuing.
4. Update completed tasks to `completed` via TaskUpdate. If a task failed or reported a deviation, mark it with a comment describing the issue and continue with the next batch (dependent tasks will remain blocked).
5. **Git checkpoint**: If there are subsequent batches that depend on this one, stage and commit the current batch's changes before proceeding. This makes failures in later batches revertible without losing earlier work.
6. **Rollback on batch failure**: If a batch fails and cannot be fixed within the retry budget (see below), `git revert` to the last successful batch commit. Report the revert and the failure reason so the user can update the plan.

### Retry budget

When a task fails (build error, test failure, agent-reported issue):
- **Maximum 2 fix attempts per failure.** Each attempt gets a targeted fix agent with the specific error and file context.
- After 2 failed attempts, mark the task as failed, revert its changes if they break the build, and continue with unaffected tasks.
- Report all failures and attempted fixes in the Phase 4 summary.

### Handling cross-cutting changes

If a change spans many files (e.g. renaming an interface used in 15 places):
- Do NOT split across multiple agents — give it to a single agent with the full file list.
- If the file list is too large for one agent, split into sequential batches (batch 1: change the definition + direct consumers, batch 2: change indirect consumers).

## Phase 3: Verify

After all batches complete, launch a **verification sub-agent** (keeps verbose build/test output out of the main context):

The verification agent MUST:
- **Use the verification commands from the plan** if they were extracted in Phase 1. Do not re-discover commands that are already known.
- If no commands were provided from the plan, determine the project's build and test commands by checking: (a) CLAUDE.md for documented commands, (b) project root files (e.g. Cargo.toml, package.json, *.sln, Makefile, pyproject.toml). If ambiguous, ask the user.
- Run the appropriate build commands
- Run relevant tests
- If builds or tests fail, report the specific errors with file paths and line numbers
- Return a concise summary — not the full build/test output

If verification fails:
1. **Reason thoroughly to diagnose** in the main conversation. Thoroughly analyse the failure and determine root cause.
2. Fix the issue directly or launch a targeted fix agent. **This counts against the retry budget** — maximum 2 fix-and-reverify cycles for the entire verification phase.
3. Re-run verification.
4. If verification still fails after 2 attempts, report the specific failures and suggest the user investigate manually or update the plan.

## Phase 4: Report

**Reason thoroughly through the final report.** Cross-reference all agent results, verify completeness against the original plan/task, and ensure the summary accurately reflects what was done.

After successful verification, output:

```
## Implementation Summary

### Completed
- [task] — files changed, what was done

### Failed / Skipped
- [task] — reason, what needs manual attention

### Plan Deviations
- [task] — what the plan assumed vs. what was found, and how it was handled (adapted / deferred / reverted)

### Verification
- Build: pass/fail
- Tests: pass/fail (N passed, M failed)
- Fix attempts used: N/M

### Plan Updates Needed
- [items completed — run `/plan-update status` to record]
- [deviations from plan — run `/plan-update deviation` to record]
```

### Phase 4.5: Sync plan context

After the Implementation Summary has been emitted, synchronise the resolved flow's `context.toml` with the work just completed.

1. **No-op gate**: if `[tasks].in_progress == 0` in the resolved flow's `context.toml` AND no files under its `scope` were edited during this run, skip the invocation entirely and note the skip in the orchestrator's output ("Phase 4.5 skipped: no-op gate"). This prevents spurious `plan-update` calls on trivial or inline runs that never touched tracked scope.
2. **Otherwise, auto-invoke `plan-update`**: use the `Skill` tool to call the `plan-update` skill with the literal string argument `status`. The skill will read the resolved flow's `context.toml`, update `[tasks]` counters to reflect what the Implementation Summary reported, set `updated` to today, and preserve `created` verbatim.

Because `plan-update` itself performs the 5-step flow resolution, no flow arguments need to be passed through — the invocation is literally `Skill("plan-update", "status")`.

## Important Constraints

- **Context budget** — Be selective about what you read in Phase 1. Agents have full tool access and will read their own target files, so the orchestrator doesn't need to pre-read every file. This is especially important when commands are chained (e.g. `/implement ... then /review then /implement fixes`) — reserve context for later phases.
- **Front-load complex analysis in Phase 1** — the orchestrator has the broadest view, pre-digested instructions let agents execute rather than re-deliberate, and complex reasoning is verified once rather than N times. Give agents pre-digested instructions, not open-ended problems.
- **3-4 parallel implementation agents max** — more creates coordination overhead. Research-only agents can scale higher.
- **File ownership is absolute** — no two parallel agents touch the same file. Sequence if necessary.
- **Commit between dependent batches** — so later failures don't require reverting earlier successes.
- **Preserve existing patterns** — agents must read surrounding code and match style, naming, structure.
- **Do not over-implement** — make the minimum changes to satisfy each task. No bonus refactoring.
- **Verification is mandatory** — never report success without running build + tests.
- **Retry budget is strict** — maximum 2 fix attempts per task failure, maximum 2 fix-and-reverify cycles for verification. After that, report and move on.
- **Plan deviations surface immediately** — agents report mismatches between plan and reality rather than silently adapting. The orchestrator decides whether to proceed, fix, or abort.
