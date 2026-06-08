---
description: Implement a plan or task using parallel sub-agents with research, progress tracking, and verification
argument-hint: [--flow <slug>] [plan path or task description]
---

# Implementation

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.

Implements a plan, feature, or task by delegating to parallel sub-agents — work decomposition, research for novel steps, efficient parallelisation, progress tracking via Task tools, and verification. Accepts plan files, plan directories, specific items (`items 3,4,5 from …`), inline task descriptions, or no arguments (auto-resolves the active flow via the Step-0 envelope).

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning, tool usage, and deviation detection.

## Step 0: Pre-flight (flow resolution + doctor)

Invoke the `flow-contract-flow-context` skill to load the flow-bootstrap envelope contract (input/output shapes, `envelope.ok` gating, `envelope.resolved.*` and `envelope.doctor.*` binding rules, no-flow fallback, doctor-fail handling, staleness reconciliation, and the mandatory bootstrap-summary console line).

Build the input envelope:

```bash
tomlctl flow envelope build \
  --command implement \
  --branch "$(git branch --show-current)" \
  --worktree "$(git rev-parse --show-toplevel)" \
  --cwd "$(pwd)"
```

On detached HEAD, omit `--branch` so the envelope records `branch:null`. Pass `--flow-override <slug>` when the user supplied `--flow`, and `--path-arg <p>` once per `$ARGUMENTS` path token. Set `require_artifacts = ["execution_record"]` and `staleness_threshold = "7d"`. Dispatch `flow-bootstrap` via the Task tool with `subagent_type: "flow-bootstrap"` and the printed JSON as the prompt. Gate on `envelope.ok`; bind `slug`, `context_path`, `artifacts.*` (esp. `execution_record`), and `doctor.ok` for downstream phases. Emit the bootstrap-summary line before any other action. On no-flow, prompt the user per `envelope.warnings` / `tie_candidates`.

## Phase 1: Analyse and Decompose (main conversation — thinking enabled)

**Reason thoroughly.** Front-load analysis here. Read the resolved flow's `context.toml`, extract `plan_path`, and read the plan.

**Plan-path validation (mandatory, before any plan Read).** The plan is embedded verbatim into every Phase-2 agent prompt. Resolve the candidate path and verify it falls under the git top-level. **Reject** if: (1) it contains `..` after normalisation; (2) it is absolute and not under the git top-level prefix (`/tmp`, `/etc`, `~/`, etc.); (3) it resolves outside the repo via symlink. Halt naming the offending path; dispatch no agent. This binds to the initial read, outline/detail reads, and any Phase-4.5 re-read.

Handle plan-directory (start at outline/master, then only relevant detail docs), single-file, inline-task, and item-subset (`items 3,4,5`) inputs. Update the resolved `context.toml`: read pre-update `status` as `<old_status>`, set `status = "in-progress"`, set `updated` to today, increment `[tasks].in_progress`, preserve `created` and key order. `[tasks].in_progress` is live-TaskCreate state — `/plan-new` / `/plan-update` never touch it.

Invoke the `flow-contract-execution-record-schema` skill to load the canonical execution-record contract (schema, type vocabulary + per-type required fields, the two-call heredoc write contract, `<record>` path-resolution rule, `[[items]]` subcommand restrictions, append-only/supersession, render-from-log routine, `[tasks].completed` derivation, read-path `--verify-integrity` integrity contract, field-length caps, and read rules). Resolve `<record>` as `[artifacts].execution_record` (fallback `.claude/flows/<slug>/execution-record.toml`); if absent on disk, bootstrap atomically with one `Write` of `schema_version = 1\nlast_updated = <today>\n`. Use `<record>` (fully-qualified) for every later `tomlctl` call — never the bare filename. If `<old_status> != "in-progress"`, append a `type=status-transition` entry per the skill's heredoc form (skip the no-op case). Build the idempotency skip-list (`tomlctl items list <record> --where type=task-completion --where status=done --pluck task_ref --verify-integrity`); skip any plan task whose slug matches. Extract `## Verification Commands` for Phase 3.

Research novel/complex steps now (Context7 + WebSearch), resolve ambiguities, decompose into discrete non-overlapping-file tasks (classify Straightforward vs Complex), identify dependencies, target 3-4 parallel agents max, and create TaskCreate entries with `addBlockedBy`.

## Phase 2: Execute (parallel sub-agents)

Launch implementation agents in dependency-ordered batches; each batch runs in parallel, batches run sequentially. **Every agent in a batch MUST be emitted in the same assistant response** — N tasks ⇒ N `Agent` blocks before the turn ends, no fewer. A second `Agent` call on a later turn serialises the batch. Do NOT reduce the agent count. Place shared context as a byte-identical literal preamble atop each agent prompt (prompt-cache reuse), with per-agent divergence below a divider.

**Agent dispatch rules.** Each prompt MUST include: exact files (absolute paths); "read every listed file in full plus any file you import/export"; what the code should do and why; research findings for complex tasks; specific API signatures; success criteria; mandatory Context7/WebSearch verification; step-by-step reasoning; and the plan-deviation protocol ("if the plan's assumptions are wrong, do NOT silently improvise — complete unaffected changes, report what was assumed vs found vs left undone"). Include Context7/WebSearch/codebase-exploration/diagnostics tool guidance tailored per task. **No whole-suite self-verification:** each prompt MUST also direct the agent NOT to run the full build or whole test suite to self-check (`cargo build` / a bare `cargo test` / `cargo nextest` / `bun run build`) — those are the orchestrator's job at the batch checkpoint (Phase 2 step 5) and the final pass (Phase 3). If the task's `Acceptance` names a whole-suite command, the agent treats it as the orchestrator's checkpoint responsibility, not its own; a delegate may still run a cheap check (`cargo check` / `cargo clippy` / `bun run type-check`) or its task's own narrow test (`cargo test --test <name>` / `cargo nextest -E 'test(<area>)'`) before handoff.

**Lite-eligibility gate (per-task dispatch).** Pick `implement-deep` (default) or `implement-lite`; dispatch `-lite` ONLY when ALL four criteria hold for EVERY task in the batch. Effort tag is primary: `S` is lite-eligible subject to the criteria; `M`/`L` ALWAYS go deep (do not evaluate the rest); no/unknown tag ⇒ treat as `M`. The four criteria (S only): (1) ≤ 2 files; (2) action fully specified, no design decisions left; (3) no cross-file refactor; (4) not security-sensitive (auth/crypto/input-validation/sandbox/token-storage). Coupling-isolation: if ANY task is `M`/`L` or fails any criterion, the WHOLE batch goes deep — do not peel tasks out. Record the choice as a one-line `DISPATCH:` header atop each agent prompt.

For each batch: TaskUpdate all to `in_progress`; dispatch the whole batch in one response (re-check the N-blocks-emitted invariant before ending the turn). On return:

- **3a. Vet `implement-lite` output before promoting completion.** Inspect every `applied <id> [vet-recommended]` tag (read touched files, confirm soundness); spot-sample ≥ 1 bare `applied` per lite cluster (confirm match to `Action`+`Detail`, style preserved). Sample-failure ⇒ expand to 100% of that cluster and re-dispatch failed items to `implement-deep` (counts as a failed retry). Skip vetting for deep clusters. Do NOT append a `task-completion` for a task whose vet failed.
- **3b. Plan deviations.** Per detected deviation, append a `type=deviation` entry to `<record>` (skill heredoc form; required `original_intent`, `rationale`, `commits`). **Pre-append dedupe guard (mid-batch-crash safety):** query for an existing match on the `(task_ref, original_intent, rationale)` triple via `tomlctl items list <record> --where type=deviation … --count`; append only when count is 0 (or dedupe on a `deviation_fingerprint` hash). Significant deviations pause and surface to the user. Do NOT advise a second `/plan-update deviation` for an already-recorded deviation.
- **4.** TaskUpdate completed tasks; failed/deviation tasks get a comment and execution continues (dependents stay blocked). Before `git commit`, apply the `commit-conventions` skill if installed and emit the sentinel `IMPLEMENT-AUTOCOMMIT: phase-2-step-5`.
- **5. Checkpoint gate + commit.** When this batch will be checkpoint-committed (a subsequent batch depends on it), **gate the commit on a build+test pass** so the commit is non-broken and bisectable. Dispatch the `verification` agent (`subagent_type: "verification"`, Haiku) with the checkpoint command list: for each crate the batch touched, build it then run its full test suite — `cargo build --manifest-path <crate>/Cargo.toml` then `cargo nextest run --manifest-path <crate>/Cargo.toml` (or `cargo test`); for SPA-touching batches, `bun run build` then `bun test`. It short-circuits on first `fail`. On `fail`: diagnose in the main conversation and fix directly or via a targeted agent (counts against the retry budget — max 2 fix-and-reverify cycles), then re-run; **do NOT commit a red batch** — if it cannot go green within budget, go to step 6. On green: append one `type=verification` entry per executed command (skill heredoc form; `command`, `outcome`; `task_ref` = the batch's lead task), then stage+commit before the step-5b append so the entry carries the real SHA (`git rev-parse HEAD`). **If `git commit` fails (e.g. a pre-commit hook rejects the change): do NOT proceed to step 5b — the task is not complete. Surface the hook failure to the user and halt; the task-completion entry must not be appended for an uncommitted terminal state.** If no dependent batch follows, skip both the gate and the commit (entry carries empty `commits[]`) — the Phase-3 final pass covers that batch.
- **5b.** Per terminal-state task, append a `type=task-completion` entry (skill heredoc form). Required: `task_ref`, `status ∈ {done,failed,skipped}`, `files` (verbatim from agent, after the filter below), `commits`, `dispatch_tier ∈ {lite,deep}`, `dispatch_agent ∈ {implement-lite,implement-deep}` (tier↔agent invariant). **Path validation for `files[]` (before the add):** reject entries beginning with `/`, `\\`, or `~` and any with `..` components — drop them with a console warning; if the array empties, halt with `"Phase 2 step 5b refused to persist task-completion for <task_ref> because all reported files[] failed validation"` and append nothing (rerun picks it up via the skip-list). Out-of-`scope` paths get a soft warning + `scope_warning = true`, not a reject. **Free-text JSON sanitisation:** RFC-8259-encode every agent-supplied free-text field (`task_ref`, `summary`, `rationale`, `original_intent`, `reason`, `reevaluate_when`) — escape `\\`, `"`, U+0000–U+001F, U+2028, U+2029 — for every JSON-payload heredoc in `/implement`. Conclude every two-call write with `tomlctl set <record> last_updated <today>`.
- **6. Rollback on batch failure.** If a batch can't be fixed within the retry budget, `git revert` to the last successful batch commit and report. **Mixed-success / partial-write batches:** attempt step-3a per-failed-item re-dispatch to `implement-deep` FIRST — do NOT `git revert` the whole batch (that undoes successful agents). Only after re-dispatch also exhausts the budget does `git revert` fire, scoped to the failing items' files; successful items retain changes.

**Retry budget:** max 2 fix attempts per failure; after that mark failed, revert if it breaks the build, continue. **Cross-cutting changes:** give a 15-file rename to one agent (never split); if too large, sequence (definition+direct consumers, then indirect).

**Stash escalation handler (delegate-emitted `stash-required`).** Delegates may NOT run `git stash`/`reset`/`checkout --`; they return `escalate <id>: stash-required — <reason>` and exit. As orchestrator: (1) halt the batch and wait for running siblings to terminate (concurrent stash is unsafe — strictly serial); (2) `git stash push -u -m "implement-escalation-stash-<ISO timestamp>"`, capture the stash ref (skip to step 4 if `No local changes to save`); (3) perform the requested observation (typically a `Read` of the now-clean on-disk state — no new edits); (4) `git stash pop <stash-ref>` — **if `git stash pop` reports a merge conflict, do NOT auto-resolve — surface the conflict to the user with the literal stash ref and halt the run (do not proceed to step 5); the stash remains on the stack for user recovery**; (5) re-dispatch the delegate with a "Stash escalation context" preamble (counts one retry attempt); (6) surface the event in the Phase-4 report. Never `git checkout --`/`restore` (discards work without recovery); re-derive working-tree state via `git status --porcelain`; a handler that cannot satisfy the escalation terminates the task `failed` — do not loop.

## Phase 3: Verify

Determine verification commands (Phase-1 extraction, else CLAUDE.md / project-root manifests; ask if ambiguous). This is the FINAL verification tier — each dependent-batch checkpoint (Phase 2 step 5) already gated build + the touched crate's tests, so Phase 3 runs the full ordered suite across all touched crates plus lint + audit. Launch the `verification` agent once (`subagent_type: "verification"`, Haiku) with the full ordered `commands:` list (build → tests → lint); it short-circuits on first `fail`. Per command actually executed, append one `type=verification` entry to `<record>` (skill heredoc form; required `command`, `outcome ∈ {pass,fail}`); conclude with one `tomlctl set <record> last_updated <today>`. On failure: diagnose in main conversation, fix directly or via targeted agent (counts against budget — max 2 fix-and-reverify cycles), re-run. A re-run for the same `(command, task_ref)` MUST set `supersedes_entry` to the prior verification id (query last id first). **End of Phase 3:** invoke the skill's render-from-log routine to regenerate `PROGRESS-LOG.md` from `<record>` (cheap, idempotent — guards against the Phase-4.5 no-op gate skipping the render).

## Phase 4: Report

**Reason thoroughly.** After successful verification, output the Implementation Summary:

```
## Implementation Summary

### Completed
- [task] — files changed, what was done

### Failed / Skipped
- [task] — reason, what needs manual attention

### Plan Deviations
- [task] — plan assumption vs. what was found, how handled (adapted / deferred / reverted)

### Verification
- Build / Tests / Lint: pass/fail; fix attempts used: N/M
### Next Steps
- review the revised plan + PROGRESS-LOG.md; then /review + /optimise on the scope, then /plan-update <slug> complete
```

### Phase 4.5: Sync plan context

1. **No-op gate:** if `[tasks].in_progress == 0` AND no scoped files were edited, skip and emit `Phase 4.5 skipped: no-op gate (in_progress=<N>, scoped-edits=<count>)`.
2. **Otherwise** call the `plan-update` skill with literal arg `status` (`Skill("plan-update", "status")`); it refreshes `[tasks]` counters, sets `updated`, preserves `created`, re-renders `PROGRESS-LOG.md`, and MAY transition to `review` but MUST NOT transition to `complete`.
3. When `status` is now `review`, append the one-line hint `flow <slug>: implementation complete — status is now "review". Run /review and /optimise against the scope, then /plan-update <slug> complete to drop the flow from auto-resolution.` (interpolate `<slug>`).

## Important Constraints

- **Context budget** — be selective in Phase 1; agents read their own targets. **Front-load complex analysis** — give agents pre-digested instructions, not open-ended problems.
- **3-4 parallel implementation agents max**; file ownership is absolute (no two parallel agents touch one file — sequence if needed). **Commit between dependent batches**; preserve existing patterns; do not over-implement.
- **Verification is orchestrator-owned, never per sub-agent** — it runs at two tiers: a per-checkpoint gate (build + the touched crate's tests, blocking each checkpoint commit) and the final full pass (build + tests + lint + audit). Delegates do not self-verify with whole-suite builds/tests; never report success without the final pass. **Retry budget is strict** — max 2 fix attempts per task failure, max 2 fix-and-reverify cycles for verification.
- **Plan deviations surface immediately** — agents report mismatches; the orchestrator decides proceed/fix/abort.
