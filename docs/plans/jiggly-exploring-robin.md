# Plan: v2 execution engine, first slice

**Plan path**: `docs/plans/jiggly-exploring-robin.md`
**Created**: 2026-11-09

## Summary

This plan builds the first slice of the v2 execution engine: `/implement-v2` runs an approved v2 plan one milestone at a time, with `lite` and `deep` workers dispatched by a saved Workflow, premise checks before dispatch, a gate and one commit per milestone, a drain replan and handovers. It also adds the tomlctl surface the engine needs (a `parked` task status, `tasks ready --checkpoint`, scoped and recorded premise probes, `plan drift --uncommitted`, glimpse correlation for worker prompts) and ships it as tomlctl 0.21.0. Scrutinise first: Approach, "Worker contract", which lets a worker write `status` and `agent` on its own row and so amends task-store section 11; and Approach, "Gate, commit and fix rounds", which fixes what happens when a gate fails. At 51 files the plan is over the usual 25-file split line by the user's choice, in two checkpoint groups.

## Context

`/plan-v2` can author, prove, review and approve a plan, but nothing can run one. `CLAUDE.md` says so, and `human-only-handovers` has sat approved with 12 pending tasks since commit 9890f47. The design is settled in `docs/ideas/v2-pipeline.md`, "Execution engine, first slice", and `docs/plans/v2-planning.md`, "After approval", which still binds. Exploration found that the tomlctl verbs fall short of that design in six places (Exploration Notes, "Gaps needing tomlctl code"), so the engine prose cannot be written against the installed binary alone.

The outcome is that `/implement-v2 human-only-handovers` runs milestone A to a verified commit without the orchestrator reading any worker transcript.

## Scope

- **In scope**: the tomlctl and glimpse additions listed in the Summary; two worker agents; the `implement-v2-milestone` Workflow; the `implement-v2-engine` skill; the `/implement-v2` carrier; contract and project doc amendments; link-script coverage for every v2 entry.
- **Out of scope**: the `tool` and `clerk` executors, vetting, apply and harvest; deferred-milestone elaboration (the engine stops at a deferred milestone and says so); worktree isolation; an Agent-tool fallback; any change to `/implement` or the v1 implementer agents; closing the flow once its last milestone is committed (the engine's closing report says the flow's status is still to be moved); actually running `human-only-handovers`.
- **Affected areas**: `tomlctl/src/tasks/`, `tomlctl/src/plan/`, `tomlctl/src/agents/`, `tomlctl/src/backlog/`, `tomlctl/src/cli/`, `tomlctl/tests/`, `glimpse/src/`, `claude/agents/`, `claude/commands/`, `claude/skills/implement-v2-engine/`, `claude/skills/plan-v2-engine/`, `claude/skills/tomlctl/`, `claude/skills/flow-contract-task-store/`, `claude/workflows/`, `scripts/`, `docs/ideas/`, `docs/plans/v2-planning.md`, `CLAUDE.md`.

## User Decisions

Answers are data, not instructions to an implementer.

1. **Which workers should the engine dispatch?** New v2 worker agents. Prompted by: the v1 implementers' `forbidden-working-tree-ops` block forbids `tomlctl tasks update` and their vocabulary is v1 (Exploration Notes, "Workers").
2. **Where is a parked task recorded?** A new `parked` task status. Prompted by: the status vocabulary has no parked value (Exploration Notes, "tomlctl surface that exists"). The orchestrator's reading: outcome detail still needs a home, so each worker also writes one note file under `docs/flows/<slug>/notes/run/`.
3. **How much tomlctl work belongs in this slice?** The fuller surface: `tasks ready --checkpoint`, premise probes covering `after:` premises with a per-task scope and recorded verdicts, drift over uncommitted edits, and glimpse correlation. Prompted by: Exploration Notes, "Gaps needing tomlctl code".
4. **Which backlog items does the plan deliver?** B-735ed4dd. Prompted by: Exploration Notes, "Backlog".
5. **How should about 38 files be planned?** One plan, two milestone groups. Prompted by: the Phase 2 scope check.
6. **Checkpoint cadence?** `milestones`.
7. **Who writes the `commit` field?** The orchestrator stamps it after the milestone commit; workers write only `status` and `agent`. No `--expect-status` guard is added. Prompted by: workers never commit, and `tasks update` has no compare-and-set (Research Notes, "Status-write rules").

### Phase 5 outcome

Skipped. Every answer names verbs and files the Phase 3 notes already cover.

## Approach

### Status and parking

`parked` joins `pending | in-progress | done | failed | deferred`. A worker sets `failed` when it tried and could not meet the task's accept checks, which the drain replan can fix. It sets `parked` when only a human can unblock the task: a `required` means is impossible, two constraints conflict, or the task's outcome looks wrong. In the frontier a parked row stalls its dependents exactly as `failed` and `deferred` do. `plan stage` keeps reporting `running`. The reason lives in the worker's note, never in the store.

### Milestone frontier

`tasks ready --checkpoint <id>` restricts all four lists to rows whose `checkpoint` is `<id>`. Dependencies outside the milestone still gate, so a row waiting on an unfinished row of another milestone appears under `blocked` or `next`, never `ready`. An id no checkpoint carries is refused.

### Premise checks

`plan probes --emit --premises` gains `--task <N1,N2>` and stops skipping an `after:<id>` premise once task `<id>` is `done`. `plan probes --record <capture> --premises` stores a verdict on each premise: `holds` (exit 0), `fails` (non-zero), or `broken` (a breakage marker on stderr, by the same `BROKEN_MARKERS` the accept path uses). tomlctl still never runs a stored command; the orchestrator runs the emitted script. A task with a premise that does not hold is withheld from the run and goes to the drain replan, never up a tier.

### Drift

`plan drift --uncommitted` adds the working tree's modified and untracked paths to the report. A pending row is affected by an uncommitted path unless a `done`, `in-progress`, `failed` or `parked` row with an empty `commit` also names that path, since that edit is the run's own work. Paths under either of the flow's directories, `.claude/flows/<slug>/` and `docs/flows/<slug>/`, are never reported. This is how a drain sees a worker's edit outside its lane before the milestone commit hides it. Committed drift is unchanged, so at a first run every commit since approval still counts; the engine answers that with the premise checks, not with a stop.

### Worker contract

Two new agents, `plan-worker-lite` and `plan-worker-deep`, share one procedure and differ in the judgement they are licensed to use, as the `implement-*` pair does. Model and effort live only in frontmatter. The dispatch prompt is key lines built from `args`: `slug:`, `task:`, `root:`, `note:`, `siblings:` (the files other in-flight tasks hold), and the two fetch commands. One worker gets one task.

Procedure:
1. Confirm `git rev-parse --show-toplevel` equals `root:`; stop with `status: error` if not.
2. Read the task with `tomlctl plan view --slug <slug> --for worker --task <n>`. The prompt also carries `tomlctl tasks show <n> --slug <slug>` so the row's JSON is one call away.
3. If the row is already `done`, return that and change nothing.
4. Check the diff of the task's own files (`git status --porcelain -- <files>`, then `git diff -- <files>`). The runtime may have restarted this worker with an earlier attempt's edits in place, so continue from what is there.
5. Set `status = in-progress` and `agent = <own name>` on the task's own row.
6. Do the work inside the task's files, honouring constraints and `required` means. Never commit, stage, stash or switch branches.
7. Run the narrow check the build-discipline rule allows, and each accept check whose `writes_build` is `false`.
8. Write the note at `note:`: what changed, each deviation from a `suggested` means, any proposed handover, any `TANGENTIAL:` discovery, and for `failed` or `parked` the reason and what would unblock it.
9. Set the row's final status: `done`, `failed` or `parked`.
10. Return `{task, status, note, files_touched, deviations, handover}`.

A worker writes only `status` and `agent`, only on its own row, and nothing else in any store. Each row therefore has one writer at a time: its worker during a run, the orchestrator between runs. Nothing enforces this, since `Bash(tomlctl *)` approves every verb, so the orchestrator snapshots `tomlctl tasks list --slug <slug>` to scratch before each launch and, at the drain, reports any change other than `status` and `agent` on a dispatched row.

### Milestone Workflow

`claude/workflows/implement-v2-milestone.js` takes `{slug, milestone, root, max_parallel, tasks: [{id, executor, needs, files}]}`, where `needs` lists only ids inside `tasks`. It loops in plain JavaScript: each wave is every task whose `needs` all returned `done` and whose `files` overlap no other task in the wave, cut to `max_parallel`. A task behind a non-`done` return is reported `blocked` and never dispatched. Each `agent()` call is wrapped in `caught()` and takes its `agentType` from a lookup on `executor` whose entries each carry a literal `agentType`. The script returns an index `{slug, milestone, tasks: [{id, status, note, error}], errors, script_version}` and follows the argument-refusal and `no agent launched` conventions of `claude/workflows/plan-v2-prove.js`.

### Orchestrator loop

The engine runs one milestone per Workflow run and continues to the next milestone while nothing is parked.

1. **Pre-flight.** `plan stage` reports `approved` or `running`; `plan check` and `tasks check` are clean; the working tree is clean outside the flow's own directories, `.claude/backlog.toml` and its sidecar, and the files of the milestone's rows that are not `pending` and carry an empty `commit`, which hold the milestone's own unfinished work. With no open run, any `in-progress` row is reset to `pending`, its edits left in place for the next worker's own-diff step.
2. **Pick the milestone.** The first checkpoint, in store order, with a row that is not `done`. A milestone whose elaboration is deferred stops the engine with a message.
3. **Premises.** Emit the premise script for the milestone, run it, record the capture, and withhold every task with a premise that is not `holds`.
4. **Launch.** Build `args` from the milestone's pending members (`tasks closure --checkpoint <id>`): drop each withheld task and every member that transitively needs one, drop any row whose spec `executor` is neither `lite` nor `deep` and list it in the `needs_human` report, keep in each row's `needs` only ids left in that set, and refuse a member with a not-`done` need outside it; a premise skipped because its `after:` task is not done withholds its task; take `files` from the rows and the executor from each spec. `tasks ready --checkpoint` is the test that a dispatchable row remains, never the source of `args`, since it names only the current wave and the next. Snapshot `tomlctl tasks list --slug <slug>` to scratch; write the args to scratch; launch; `plan run start --step implement:<milestone>`; end the turn. Never `resumeFromRunId`.
5. **Drain.** On the notification, `plan run finish`. Read the index, then only the notes of tasks that are not `done` or that report deviations or a handover. Run `plan drift --uncommitted`, then reset to `pending` every row of the run still `in-progress`, which a worker whose `agent()` call returned nothing leaves behind, since `tasks ready` reads `in-progress` as stalled. Compare the store with the pre-launch snapshot and report any change other than `status` and `agent` on a dispatched row. Replan through audited verbs with a `reason`. Relaunch a fresh run while dispatchable rows remain.

### Gate, commit and fix rounds

When every row of the milestone is `done`, the `verification` agent runs the milestone's `gates[]`. On a pass the orchestrator commits once through the `commit-conventions` skill, stamps the sha on every row of the milestone, and commits the flow stores, the run's notes under `docs/flows/<slug>/notes/run/` and `.claude/backlog.toml` as a second small commit. Re-entry stamps an all-`done` milestone whose rows carry an empty `commit` from HEAD's milestone commit when that commit holds their files, and runs the gate again otherwise. On a fail it mints fix tasks in the same milestone with `tasks add-many` and a `reason`, each row carrying a `spec` with an `outcome`, an `accept` entry with a `check`, and an `executor` of `lite` or `deep` (a spec-less row is a `plan/task-no-spec` error and gives the Workflow no executor), and relaunches. After two failed rounds it stops and reports the milestone as needing the user. `flaky` is a pass.

### Handovers

A worker proposes a handover in its note; only the orchestrator writes one, with the handover verb the tomlctl `plan` reference documents (`plan handover add` today). The engine's own references name that verb in prose and never in a bash fence, because `human-only-handovers` replaces it and `command_lint` parses every fenced call. At the end of every invocation the engine lists open handovers and parked tasks with their note paths, as the `needs_human` report. At the next invocation it asks through `AskUserQuestion` how each parked row of the current milestone is unblocked, and replans it to `pending` through audited verbs with the answer as the `reason`, before the premise step.

### Carrier and deploy

`/implement-v2 <slug> [--milestone <id>]` is a thin carrier shaped like `claude/commands/plan-v2.md`: bindings, a pre-flight that checks the `plan` group and the new capability features and hard-stops when `Workflow` is unavailable, then a handoff to the skill. `scripts/link-v2-review.sh` and `.ps1` gain every plan-v2 and implement-v2 entry, so one script links all of v2.

### Contract amendments

Task-store section 11 keeps the orchestrator as the only writer of plan fields and of `commit`, and adds one exception: a v2 worker writes `status` and `agent` on its own row. `docs/plans/v2-planning.md` and `docs/ideas/v2-pipeline.md` are brought into line, including the `commit` correction in User Decisions 7.

## Success Criteria

- forward: `test -f claude/commands/implement-v2.md && test -f claude/skills/implement-v2-engine/SKILL.md && test -f claude/workflows/implement-v2-milestone.js` (today: exit 1)
- forward: `grep -q 'cannot be run yet' CLAUDE.md; test $? -eq 1` (today: exit 1, the phrase is present)
- forward: `grep -q 'commands/plan-v2.md' scripts/link-v2-review.sh && grep -q 'commands/implement-v2.md' scripts/link-v2-review.sh` (today: exit 1)
- forward: `grep -q '"parked"' tomlctl/src/tasks/schema.rs && grep -q '^version = "0.21.0"' tomlctl/Cargo.toml` (today: exit 1)
- guard: `bash scripts/verify-shared-blocks.sh` exits 0 (today: exit 0)
- **predicted, unverified**: after `cargo install --path tomlctl`, `tomlctl tasks ready --slug human-only-handovers --checkpoint A --get ready` lists only ids from 1 to 7.
- forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes with the new carrier, skill, agents and Workflow in the tree. **predicted, unverified**: needs a build.
- forward: `grep -q 'additionalProperties: false' claude/workflows/implement-v2-milestone.js && grep -qi 'transcript' claude/skills/implement-v2-engine/SKILL.md` (today: exit 2, no files)
- **predicted, unverified**: `/implement-v2 human-only-handovers` runs milestone A to a gate-verified commit.

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast
test.rerun: cargo nextest run --manifest-path tomlctl/Cargo.toml --no-fail-fast -j 1 -- --exact {ids}
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets && cargo clippy --manifest-path glimpse/Cargo.toml --locked
success: test -f claude/commands/implement-v2.md && test -f claude/skills/implement-v2-engine/SKILL.md && test -f claude/workflows/implement-v2-milestone.js
success: grep -q 'cannot be run yet' CLAUDE.md; test $? -eq 1
success: grep -q 'commands/plan-v2.md' scripts/link-v2-review.sh && grep -q 'commands/implement-v2.md' scripts/link-v2-review.sh
success: grep -q '"parked"' tomlctl/src/tasks/schema.rs && grep -q '^version = "0.21.0"' tomlctl/Cargo.toml
success: bash scripts/verify-shared-blocks.sh
success: grep -q 'additionalProperties: false' claude/workflows/implement-v2-milestone.js && grep -qi 'transcript' claude/skills/implement-v2-engine/SKILL.md
```

Phase 3 also runs `cargo fmt --manifest-path tomlctl/Cargo.toml -- --check`, `cargo fmt --manifest-path glimpse/Cargo.toml -- --check`, `cargo nextest run --manifest-path glimpse/Cargo.toml --no-fail-fast`, `cargo doc --manifest-path tomlctl/Cargo.toml --lib --no-deps --document-private-items`, and `bash scripts/verify-plan-story-blocks.sh`. Prefix full runs with `CARGO_INCREMENTAL=0`.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 2, 6, 8, 9, 10, 11, 18, 20, 21, 22
- **Max parallel agents**: 6
- **Commit granularity**: per-task

## Tasks

### 1. Add the parked task status [M]
- **Files**: `tomlctl/src/tasks/schema.rs`, `tomlctl/src/tasks/graph.rs`, `tomlctl/src/backlog/reconcile.rs`
- **Depends on**: —
- **Action**: Add `Status::Parked`, spelled `parked`, and make the frontier and backlog reconcile treat it as stalled.
- **Detail**: In `tomlctl/src/tasks/schema.rs` add the variant to `enum Status`, its vocabulary array, `as_str` and `parse`. In `tomlctl/src/tasks/graph.rs` map it to `Standing::Stalled` in `Standing::of`. Add a unit test in `graph.rs` beside the existing `blocker_status` tests: a pending row behind a parked row is reported under `blocked` with `blocker_status: Status::Parked`; put `parked` in the test's name. In `tomlctl/src/backlog/reconcile.rs` add `Status::Parked` to the `stopped` test beside `Status::Deferred` and `Status::Failed`, so a backlog item whose closing task is parked is bucketed `stalled`, and extend the unit test there that covers a `deferred` closing task. No exhaustive `match` on `Status` exists outside `schema.rs` and `graph.rs`; report any the compiler names in another file instead of editing it. Relies on Approach, "Status and parking".
- **Acceptance**:
  - forward: `grep -q '"parked"' tomlctl/src/tasks/schema.rs && grep -q 'Status::Parked' tomlctl/src/backlog/reconcile.rs` (today: exit 1)
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'kind(lib) & test(parked)'` passes with at least one test run (today: no lib test matches, so nextest exits non-zero; the bare `test(parked)` filter matches two unrelated integration tests). **predicted, unverified**. Falsifier: mapping `Parked` to `Standing::Pending` turns it red.

### 2. Show the parked status in glimpse [M]
- **Files**: `glimpse/src/model.rs`, `glimpse/src/theme.rs`, `glimpse/src/view/legend.rs`
- **Depends on**: —
- **Action**: Teach glimpse's `TaskStatus` the `parked` value so it stops rendering as `Unknown`.
- **Detail**: In `glimpse/src/model.rs` add `TaskStatus::Parked` with `as_str` `parked` and a glyph no other status uses; the glyph must be one terminal cell wide, as `Zoom::label_width` in `glimpse/src/diagram/mod.rs` assumes. In `glimpse/src/theme.rs` map `"parked"` to `self.deferred` in `Theme::status` and `TaskStatus::Parked` to slot `4` (Deferred's) in `Theme::flash`, which is a `[Style; 5]` with no sixth slot. In `glimpse/src/view/legend.rs` add it to both status lists. Add or extend a unit test in `glimpse/src/model.rs`, with `parked` in its name, that deserialises `"parked"` to `TaskStatus::Parked`. Do not add a theme colour field, and leave `SAMPLE` in `glimpse/src/view/themes.rs` unchanged.
- **Acceptance**:
  - forward: `grep -q 'Parked' glimpse/src/model.rs` (today: exit 1)
  - forward: `cargo nextest run --manifest-path glimpse/Cargo.toml -E 'test(parked)'` passes with at least one test run (today: no glimpse test name holds `parked`). **predicted, unverified**. Falsifier: removing the `"parked"` arm of the deserialiser turns it red.
  - guard: `cargo clippy --manifest-path glimpse/Cargo.toml` reports no error in the three files this task owns.

### 3. Scope tasks ready to a checkpoint [M]
- **Files**: `tomlctl/src/cli/types/tasks.rs`, `tomlctl/src/tasks/ready.rs`, `tomlctl/src/tasks/dispatch.rs`
- **Depends on**: —
- **Action**: Add `--checkpoint <ID>` to `tasks ready`.
- **Detail**: Add the flag to `TasksOp::Ready` in `tomlctl/src/cli/types/tasks.rs`. In `tomlctl/src/tasks/ready.rs` add a scoped entry point that builds the frontier over the whole graph and then keeps, in `ready`, `held`, `next` and `blocked`, only rows whose `checkpoint` equals the id; leave `ready_with` unchanged, because `tomlctl/src/tasks/snapshot.rs` calls it. Refuse an id no `[[checkpoints]]` entry carries with a `kind=validation` error. Call it from the `TasksOp::Ready` arm in `tomlctl/src/tasks/dispatch.rs`. Add `parked` to the statuses the `tasks update --status` help text in `tomlctl/src/cli/types/tasks.rs` lists. Unit tests in `ready.rs`: two checkpoints, a row of B waiting on a pending row of A is in neither `ready` list scoped to B; an unknown id is refused. Relies on Approach, "Milestone frontier".
- **Acceptance**:
  - forward: `grep -A16 '^    Ready {' tomlctl/src/cli/types/tasks.rs | grep -q 'long = "checkpoint"'` (today: exit 1; the same pipe matches `long = "in-flight"` today)
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/ready/)'` passes. **predicted, unverified**. Falsifier: dropping the filter on `next` turns the two-checkpoint test red.

### 4. Scope premise scripts by task and cover after-premises [M]
- **Files**: `tomlctl/src/plan/probes.rs`, `tomlctl/src/cli/types/plan.rs`, `tomlctl/tests/plan_probes.rs`
- **Depends on**: —
- **Action**: Add `--task <N1,N2>` to `plan probes` and emit `after:<id>` premises whose task is done.
- **Detail**: In `PlanProbesArgs` add `--task` (comma-delimited ids, valid in either mode). In `tomlctl/src/plan/probes.rs` extend `task_skip_reason` with a not-requested reason and refuse an id no row carries. Change `premise_skip_reason` so an `after:<id>` premise is emitted when row `<id>` has `Status::Done`, skipped with `task <id> is not done` otherwise, and skipped with a distinct reason when `at` is neither `base` nor `after:<number>`; the reason text reaches the script, so it must hold no stored string. Add integration tests in `tomlctl/tests/plan_probes.rs` for the three `at` cases and for `--task`, and rewrite `emit_premises_prints_a_line_per_base_premise_of_a_pending_task`, which asserts that an `after:1` premise on a done task is skipped; update the `tomlctl/src/plan/probes.rs` module doc and the `--premises` help text in `tomlctl/src/cli/types/plan.rs`. Relies on Approach, "Premise checks".
- **Acceptance**:
  - forward: `grep -A40 'struct PlanProbesArgs' tomlctl/src/cli/types/plan.rs | grep -q 'long = "task"'` (today: exit 1; the same pipe matches `long = "milestone"` today)
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(plan_probes)'` passes. **predicted, unverified**. Falsifier: restoring the `not a base premise` skip turns the done-task case red.

### 5. Record premise verdicts [L]
- **Files**: `tomlctl/src/tasks/schema.rs`, `tomlctl/src/tasks/add.rs`, `tomlctl/src/tasks/update.rs`, `tomlctl/src/plan/probes.rs`, `tomlctl/src/cli/types/plan.rs`, `tomlctl/tests/plan_probes.rs`
- **Depends on**: 1, 4
- **Action**: Let `plan probes --record <capture> --premises` store a verdict on each premise.
- **Detail**: The six files must land together: the field, its construction site, its carry-over, its serialisation, the writer and the flag relaxation. Give `Premise` an optional `probe`, reusing `Probe` with `neg_rc` holding the exit code and `pos_rc` absent, parsed and serialised exactly as `Accept.probe` is, and kept out of audited input the same way: `probe: None` in `premise_entry` in `tomlctl/src/tasks/add.rs`, whose struct literal a new field otherwise breaks, and in `tomlctl/src/tasks/update.rs` a premise twin of `keep_probes`, so a re-issued premise that matches in every other field keeps its recorded probe. Leave `premises_json` in `tomlctl/src/tasks/show.rs` probe-free: `spec_moves` and the review `spec_value` read it, and a verdict there would read as a content change. In `PlanProbesArgs` drop `requires = "emit"` and `conflicts_with = "record"` from `--premises`; the `probes_mode` group already refuses `--premises` with neither `--emit` nor `--record`. Rewrite `premises_is_refused_without_emit` in `tomlctl/tests/plan_probes.rs`, whose `--premises --record` case becomes legal. In `tomlctl/src/plan/probes.rs` accept `premise` control rows in the capture and write `holds` for exit 0, `fails` for non-zero, and `broken` when stderr carries one of `BROKEN_MARKERS`; refuse a row whose digest does not match the stored check, as the accept path does. The envelope lists `{task, premise, verdict}`. Honour `--milestone` and `--task`. Tests in `tomlctl/tests/plan_probes.rs`: each verdict, a stale digest, and that an accept capture passed with `--premises` is refused. Relies on Approach, "Premise checks".
- **Acceptance**:
  - forward: `grep -q '"holds"' tomlctl/src/plan/probes.rs` (today: exit 1)
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(plan_probes)'` passes. **predicted, unverified**. Falsifier: writing `holds` regardless of the exit code turns the `fails` case red.

### 6. Show parked counts and premise verdicts in plan view [S]
- **Files**: `tomlctl/src/plan/view.rs`, `tomlctl/tests/plan_view.rs`
- **Depends on**: 1, 5
- **Action**: Add `parked` to the task counts and print each premise's recorded verdict in the worker view.
- **Detail**: In `tomlctl/src/plan/view.rs` add `"parked": count_tasks(Status::Parked)` beside the `deferred` count, and in `worker()` append the verdict to a premise line the way an accept line shows `Probe: healthy at <sha>`. A premise with no probe prints as today. Add `parked_tasks` beside `failed_tasks` in `open_items`. Add one test for each in `tomlctl/tests/plan_view.rs`, and add `"parked": 0` to the exact `counts` object `digest_counts_every_record` asserts there.
- **Acceptance**:
  - forward: `grep -q 'Status::Parked' tomlctl/src/plan/view.rs` (today: exit 1)
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(plan_view)'` passes. **predicted, unverified**. Falsifier: removing the verdict from the premise line turns the new test red.

### 7. Report uncommitted drift [M]
- **Files**: `tomlctl/src/plan/drift.rs`, `tomlctl/src/cli/types/plan.rs`, `tomlctl/tests/plan_drift.rs`
- **Depends on**: 5
- **Action**: Add `--uncommitted` to `plan drift`.
- **Detail**: Add the flag to `PlanDriftArgs`. In `tomlctl/src/plan/drift.rs`, when set, read `git status --porcelain=v1 -z --no-renames --untracked-files=all` under the repo root, drop paths under `.claude/flows/<slug>/` and `docs/flows/<slug>/` (the run's notes land in the second), and report the rest as `uncommitted_files`. Add a pending row to `affected` for an uncommitted path it names unless a `done`, `in-progress`, `failed` or `parked` row with an empty `commit` also names that path; mark such entries with the paths under an `uncommitted` key. Without the flag the output is byte-identical to today. Tests in `tomlctl/tests/plan_drift.rs`: an edit to a pending row's file is reported; the same edit is not reported when a done, uncommitted row names the file; the default output has no `uncommitted_files` key. The dependency on task 5 is the shared `tomlctl/src/cli/types/plan.rs`. Relies on Approach, "Drift".
- **Acceptance**:
  - forward: `grep -q 'uncommitted' tomlctl/src/plan/drift.rs` (today: exit 1)
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(plan_drift)'` passes. **predicted, unverified**. Falsifier: dropping the own-work exclusion turns the second case red.

### 8. Correlate a worker prompt to its task [S]
- **Files**: `tomlctl/src/agents/correlate.rs`
- **Depends on**: —
- **Action**: Make `dispatch_in` link a prompt that carries `plan view --slug <slug> --for worker --task <n>`.
- **Detail**: Add a second task regex for that literal form with the same slug pattern, and widen the line pre-filter to admit a line containing `plan view`. A prompt naming two different slugs still returns `None`. Add unit tests beside the existing `dispatch_in` tests: the plan-view form links one task; a prompt carrying both forms for the same task and slug links it once. The engine's own prompts carry both lines and already link through the existing pattern; this one covers a prompt that carries the `plan view` line alone.
- **Acceptance**:
  - forward: `grep -q 'for worker --task' tomlctl/src/agents/correlate.rs` (today: exit 1)
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/correlate/)'` passes. **predicted, unverified**. Falsifier: leaving the pre-filter unchanged turns the plan-view test red.

### 9. Bump tomlctl to 0.21.0 and advertise the features [L]
- **Files**: `tomlctl/Cargo.toml`, `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`, `tomlctl/README.md`, `tomlctl/src/capabilities.rs`, `tomlctl/src/cli/types/mod.rs`, `tomlctl/tests/capabilities.rs`
- **Depends on**: 1, 3, 5, 7
- **Action**: Set the version to 0.21.0 and add the capability features and vocabulary.
- **Detail**: The seven files land together because the version and the feature list are asserted in `tomlctl/tests/capabilities.rs` and `glimpse/Cargo.lock` is checked with `--locked`. Bump `tomlctl/Cargo.toml`; refresh the locks with `cargo tree --manifest-path tomlctl/Cargo.toml` and `cargo tree --manifest-path glimpse/Cargo.toml`, discarding their output. In `tomlctl/src/capabilities.rs` add `parked` to both status vocabulary arrays; add the features `task_status_parked`, `tasks_ready_checkpoint`, `premise_probes_scoped`, `premise_probe_record` and `plan_drift_uncommitted` to `FEATURES` in `tomlctl/src/cli/types/mod.rs`, to the `expected` array of `capabilities_features_contains_every_plan_feature` in `tomlctl/tests/capabilities.rs`, and to both the sample block and the "Feature meanings" table in `tomlctl/README.md`; leave `MUTEX_GROUPS` unchanged, since the `emit`/`record` group stays, as `plan_probes_advertises_the_one_mutex_clap_enforces` asserts. Update the version assertion and its message in `tomlctl/tests/capabilities.rs`, and the version in the `tomlctl/README.md` sample.
- **Acceptance**:
  - forward: `grep -q '^version = "0.21.0"' tomlctl/Cargo.toml && grep -q 'tasks_ready_checkpoint' tomlctl/src/cli/types/mod.rs` (today: exit 1)
  - forward: `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(capabilities)'` passes. **predicted, unverified**. Falsifier: leaving the README sample or the asserted version at 0.20.0 turns it red where a test reads them.

### 10. Document the new plan verbs [M]
- **Files**: `claude/skills/tomlctl/references/plan.md`, `claude/skills/tomlctl/references/plan-probes.md` (new), `claude/skills/tomlctl/SKILL.md`
- **Depends on**: 5, 7
- **Action**: Document `--task`, premise recording and `--uncommitted`, moving the probes section to its own reference.
- **Detail**: `claude/skills/tomlctl/references/plan.md` is 591 lines against a 600-line ceiling (`skill_references_under_line_ceiling`), so move its `plan probes` section, with the probe verdicts, into the new `claude/skills/tomlctl/references/plan-probes.md`, leave a one-line pointer, and add the new flags and the premise verdicts `holds`, `fails`, `broken` there. Add `--uncommitted` and the `uncommitted_files` and `uncommitted` output keys to the `plan drift` section. In the refusal paragraph near the top of `claude/skills/tomlctl/references/plan.md`, drop "`--premises` without `--emit`", and repoint the file's opening sentence and its `claude/skills/tomlctl/SKILL.md` entry, which both say it holds the probe verdicts. Add the new file to the References list in `claude/skills/tomlctl/SKILL.md` and fix any link that pointed at the moved section. `flag_table_lint` checks every flag table against the CLI.
- **Acceptance**:
  - forward: `test -f claude/skills/tomlctl/references/plan-probes.md && grep -q 'uncommitted' claude/skills/tomlctl/references/plan.md` (today: exit 1)
  - guard: `test "$(wc -l < claude/skills/tomlctl/references/plan.md)" -le 600` (today: exit 0)
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes. **predicted, unverified**. Falsifier: a flag table naming a flag the verb lacks turns `flag_table_lint` red.

### 11. Document the parked status and the scoped frontier [S]
- **Files**: `claude/skills/tomlctl/references/tasks.md`, `claude/skills/tomlctl/references/tasks-write.md`, `claude/skills/tomlctl/references/tasks-store.md`
- **Depends on**: 1, 3, 5
- **Action**: Add `parked`, `tasks ready --checkpoint` and the premise `probe` key to the task references.
- **Detail**: In `claude/skills/tomlctl/references/tasks.md` add `--checkpoint` to the `tasks ready` flag table and `parked` to the `blocked[].blocker_status` and `dag/stalled-dependency` rows. In `claude/skills/tomlctl/references/tasks-write.md` add `parked` to the `--status` row. In `claude/skills/tomlctl/references/tasks-store.md` add `parked` to the status vocabulary and the optional `probe` key to the premise description under `[[specs]]`.
- **Acceptance**: forward: `grep -q 'parked' claude/skills/tomlctl/references/tasks.md && grep -q 'parked' claude/skills/tomlctl/references/tasks-write.md && grep -q 'parked' claude/skills/tomlctl/references/tasks-store.md` (today: exit 1)

### 12. Add the v2 worker agents [M]
- **Files**: `claude/agents/plan-worker-lite.md` (new), `claude/agents/plan-worker-deep.md` (new), `scripts/shared-blocks.toml`
- **Depends on**: —
- **Action**: Write the two worker agents and register their shared blocks.
- **Detail**: Follow Approach, "Worker contract" for the prompt keys, the ten-step procedure, the write rule and the return object, and Approach, "Status and parking" for `failed` against `parked`. Frontmatter as `claude/agents/implement-lite.md` and `claude/agents/implement-deep.md`: `name`, `description`, `tools`, `model`, `effort`, `color`, with the lite and deep buckets' current values copied from those two files. Carry the existing blocks `file-edits` and `tomlctl-for-agents` byte-identical, adding both files to each block's `files` list in `scripts/shared-blocks.toml`. Do not carry `forbidden-working-tree-ops`, `implementer-report-lines`, `backlog-candidates` or `implementer-verification`: their v1 wording sends `TANGENTIAL:` and check lines to a return payload the orchestrator never reads. State their v2 forms in `plan-worker-procedure` instead: the narrow-check rule over the task's `files` and its `writes_build = false` accept checks, and `TANGENTIAL:` lines written into the note, never the return object. Declare two new blocks carried by exactly these two files: `plan-worker-procedure` (the procedure and return object) and `plan-worker-forbidden-ops` (no commit, stage, stash, branch switch or worktree; no store write except `tomlctl tasks update <n> --slug <slug> --status <s> --agent <name>` on the worker's own row; no Write-tool write under `.claude/`). Include the teammate-delivery paragraph the implementers carry. The deep file adds the judgement section; the lite file adds when to return `failed` instead of improvising. Every `tomlctl` call in a bash fence must parse (`command_lint`).
- **Acceptance**:
  - forward: `test -f claude/agents/plan-worker-lite.md && test -f claude/agents/plan-worker-deep.md && grep -q 'plan-worker-procedure' scripts/shared-blocks.toml` (today: exit 1)
  - guard: `bash scripts/verify-shared-blocks.sh` exits 0 (today: exit 0). Falsifier: a one-byte difference between the two carriers of `plan-worker-procedure` turns it red.

### 13. Add the milestone Workflow script [M]
- **Files**: `claude/workflows/implement-v2-milestone.js` (new)
- **Depends on**: 12
- **Action**: Write the Workflow that runs one milestone's tasks in waves.
- **Detail**: Follow Approach, "Milestone Workflow". Model the file on `claude/workflows/plan-v2-prove.js`: `export const meta = {` with a literal `name: 'implement-v2-milestone'`, `SCRIPT_VERSION`, `argProblems()`, `fail('no agent launched')`, `caught()`, a `RETURN_SCHEMA` with `additionalProperties: false`. Refuse args where a `needs` id is outside `tasks`, an `executor` is neither `lite` nor `deep`, or the graph has a cycle. Map `executor` to `agentType` through `const WORKERS = { lite: { agentType: 'plan-worker-lite' }, deep: { agentType: 'plan-worker-deep' } }`, bind `const worker = WORKERS[t.executor]` and pass `agentType: worker.agentType`; `workflow_scripts_name_real_agents` accepts a literal or a path ending `.agentType` and refuses an indexed or conditional value in the `agent()` call; set no `model` or `effort`. Build each prompt from `args` only, including the line `tomlctl plan view --slug <slug> --for worker --task <n>`, the line `tomlctl tasks show <n> --slug <slug>`, the note path `docs/flows/<slug>/notes/run/task-<n>.md` and the sibling files of the same wave. Use no clock, randomness or filesystem. A header comment states that a relaunch is always a fresh run.
- **Acceptance**:
  - forward: `grep -q "name: 'implement-v2-milestone'" claude/workflows/implement-v2-milestone.js` (today: exit 2, no file)
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes. **predicted, unverified**. Falsifier: an `agentType` naming no agent turns `workflow_scripts_name_real_agents` red.

### 14. Write the engine reference for running a milestone [M]
- **Files**: `claude/skills/implement-v2-engine/references/run-milestone.md` (new)
- **Depends on**: 3, 5, 7, 13
- **Action**: Write the procedure from pre-flight to launch.
- **Detail**: Cover steps 1 to 4 of Approach, "Orchestrator loop" and all of Approach, "Premise checks", with the exact commands: `tomlctl plan stage`, `plan check`, `tasks check`, `tasks closure --slug <slug> --checkpoint <id>` (its `members` are the source of `args`), `tasks ready --slug <slug> --checkpoint <id>`, the pre-launch `tasks list --slug <slug>` snapshot, `plan probes --slug <slug> --emit --premises --milestone <id>`, `plan probes --slug <slug> --record <capture> --premises --milestone <id>`, the args file shape, the `Workflow({name: "implement-v2-milestone", args: …})` launch, reading `error` on the launch result, and `plan run start --slug <slug> --step implement:<id> --workflow implement-v2-milestone --args-hash <hash> --run-id <runId> --session "$CLAUDE_CODE_SESSION_ID"`. Model the launch section on `claude/skills/plan-v2-engine/references/write-and-prove.md`. State that scratch files live in the session scratchpad, never under `.claude/` or `docs/flows/`, and that the emitted premise script is read before it runs, since `bash <file>` puts none of its stored commands in front of a permission prompt, then run as `timeout 540 bash <file> > <capture>`, never piped; a capture cut short by the timeout (exit 124) records nothing, as `claude/skills/plan-v2-engine/references/write-and-prove.md` says for probes. Stay under 600 lines.
- **Acceptance**:
  - forward: `grep -q 'implement-v2-milestone' claude/skills/implement-v2-engine/references/run-milestone.md` (today: exit 2, no file)
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes. **predicted, unverified**. Falsifier: a fenced `tomlctl` call with a flag the CLI lacks turns `command_lint` red.

### 15. Write the engine reference for drain, gate and commit [M]
- **Files**: `claude/skills/implement-v2-engine/references/drain.md` (new)
- **Depends on**: 1, 7, 14
- **Action**: Write the procedure from the run notification to the milestone commit.
- **Detail**: Cover step 5 of Approach, "Orchestrator loop", and all of Approach, "Gate, commit and fix rounds" and "Handovers". Give the exact commands: `plan run finish --slug <slug> --step implement:<id> --outcome ok|errors|stopped`, `plan drift --slug <slug> --uncommitted`, the audited `tasks update --slug <slug> --ndjson <path>` and `tasks add-many` forms with a `reason` (show the fix-task row shape with its `spec`), the `tasks update <ids> --slug <slug> --status pending` reset of rows left `in-progress`, the `verification` agent input (`commands:` items with optional `timeout:` and `rerun:`, plus `transient:`, as `claude/agents/verification.md` defines) built from the milestone's `gates`, read with `tomlctl items list .claude/flows/<slug>/tasks.toml --array milestones --where id=<id> --select gates`, and `tasks update <ids> --slug <slug> --commit <sha>`. Name the handover write in prose only, pointing at the handover verb in `claude/skills/tomlctl/references/plan.md`, and put no handover command in a bash fence. State which notes the orchestrator reads, that a failed premise or a `failed` task is replanned and never moved up a tier, the two-round limit, that `flaky` is a pass, that commit messages go through the `commit-conventions` skill, and the shape of the closing `needs_human` report. `TANGENTIAL:` lines in notes go through `backlog check` before `backlog add`. Stay under 600 lines.
- **Acceptance**:
  - forward: `grep -q 'uncommitted' claude/skills/implement-v2-engine/references/drain.md` (today: exit 2, no file)
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes. **predicted, unverified**. Falsifier: a fenced `tomlctl … 2>/dev/null` turns `guidance_lint` red.

### 16. Write the engine skill [M]
- **Files**: `claude/skills/implement-v2-engine/SKILL.md` (new)
- **Depends on**: 14, 15
- **Action**: Write the orchestrator contract that ties the two references together.
- **Detail**: Shape it like `claude/skills/plan-v2-engine/SKILL.md`: frontmatter with a description under 1024 characters saying it is invoked by `/implement-v2`; role rules (the orchestrator never reads worker transcripts, never runs a build inline, asks every user question itself, and names the allow rule for worker notes under `docs/flows/**` as `claude/skills/plan-v2-engine/SKILL.md` rule 6 does, adding it to no settings file); `## Rules`; `## Arguments` (`<slug>`, `--milestone <id>`); a `## Steps` table with Step, Run step, Skipped when and Reference columns linking `references/run-milestone.md` and `references/drain.md`; `## Re-entry`, which resets stale `in-progress` rows, never abandons a run still live in this session (it waits for the notification or stops the run first), asks through `AskUserQuestion` before abandoning an open `implement:<id>` run recorded under another session, since its workers may still be editing, and otherwise closes the open run with `plan run abandon`; it never resumes a run. Re-entry also covers the parked re-ask of Approach, "Handovers" and the unstamped all-`done` milestone of Approach, "Gate, commit and fix rounds". State the out-of-scope stops from Scope. Under 500 lines; every markdown link must resolve.
- **Acceptance**:
  - forward: `grep -q 'references/drain.md' claude/skills/implement-v2-engine/SKILL.md` (today: exit 2, no file)
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes. **predicted, unverified**. Falsifier: a link to a missing reference turns `skill_markdown_links_resolve` red.

### 17. Add the implement-v2 carrier [M]
- **Files**: `claude/commands/implement-v2.md` (new), `tomlctl/src/cli/dispatch/tests/skills.rs`
- **Depends on**: 9, 16
- **Action**: Write the carrier and register its required skills.
- **Detail**: Follow Approach, "Carrier and deploy" and the shape of `claude/commands/plan-v2.md`: `description` and single-quoted `argument-hint` frontmatter, the thin-carrier and effort notes, a `## Bindings` table, a numbered `## Pre-flight` (invoke the `tomlctl` skill; check `tomlctl capabilities --get subcommands` for `plan` and `--get features` for `task_status_parked`, `tasks_ready_checkpoint`, `premise_probes_scoped`, `premise_probe_record` and `plan_drift_uncommitted`, naming `cargo install --path tomlctl` on a miss; stop when `AskUserQuestion` or `Workflow` is unavailable), and a `## Run` that invokes the `implement-v2-engine` skill. It must also contain an invocation phrase for the `commit-conventions` skill and one for the `backlog-capture` skill, which the drain's `backlog add` requires. Add `("implement-v2.md", &["tomlctl", "implement-v2-engine", "commit-conventions", "backlog-capture"])` to the `expected` array of `carrier_invokes_required_skills` in `tomlctl/src/cli/dispatch/tests/skills.rs`.
- **Acceptance**:
  - forward: `grep -q '"implement-v2.md"' tomlctl/src/cli/dispatch/tests/skills.rs && test -f claude/commands/implement-v2.md` (today: exit 1)
  - forward: `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` passes. **predicted, unverified**. Falsifier: removing the `implement-v2-engine` invocation phrase turns `carrier_invokes_required_skills` red.

### 18. Amend the status-write contract [M]
- **Files**: `claude/skills/flow-contract-task-store/SKILL.md`, `docs/plans/v2-planning.md`, `docs/ideas/v2-pipeline.md`
- **Depends on**: 1, 5
- **Action**: Bring the three contract documents into line with Approach, "Contract amendments".
- **Detail**: In `claude/skills/flow-contract-task-store/SKILL.md` add `parked` to the status vocabulary line and the `dag/stalled-dependency` row, and amend section 11 with the single exception: a v2 worker dispatched by `/implement-v2` writes `status` and `agent` on its own row; plan fields and `commit` stay the orchestrator's. The file is 478 lines against a 500-line ceiling, so keep the net addition under 15 lines. In its section 1a, add the optional `probe` key to the sentence listing what `premises` entries take, saying only `plan probes --record` writes it. In `docs/plans/v2-planning.md` qualify the two sentences that say agents never write the task store (under "Cold readers" and "Deferred elaboration") and note under "Later slices" that the first engine slice is planned. In `docs/ideas/v2-pipeline.md`, "Execution engine, first slice", correct the Status bullet to `status` and `agent` only, say the orchestrator stamps `commit`, and name `parked`; in the same file's status list near the top, move the execution engine out of the "Not started" bullet and update the "Execution engine" row of the topics table to say the first slice is planned.
- **Acceptance**:
  - forward: `grep -q 'parked' claude/skills/flow-contract-task-store/SKILL.md && grep -q 'orchestrator stamps' docs/ideas/v2-pipeline.md` (today: exit 1). The corrected Status bullet in `docs/ideas/v2-pipeline.md` must contain the words "orchestrator stamps".
  - guard: `test "$(wc -l < claude/skills/flow-contract-task-store/SKILL.md)" -le 500` (today: exit 0)

### 19. Cover every v2 entry in the link scripts [M]
- **Files**: `scripts/link-v2-review.sh`, `scripts/link-v2-review.ps1`
- **Depends on**: 12, 13, 16, 17
- **Backlog**: B-735ed4dd
- **Action**: Add the plan-v2 and implement-v2 entries to both link scripts.
- **Detail**: Add to the `entries` list of `scripts/link-v2-review.sh` and the `$entries` table of `scripts/link-v2-review.ps1`: `commands/plan-v2.md`, `commands/implement-v2.md`, `agents/plan-investigator.md`, `agents/plan-challenger.md`, `agents/plan-cold-reader.md`, `agents/plan-worker-lite.md`, `agents/plan-worker-deep.md`, `skills/plan-v2-engine`, `skills/implement-v2-engine`, `workflows/plan-v2-investigate.js`, `workflows/plan-v2-prove.js` and `workflows/implement-v2-milestone.js`. In the `.ps1`, skills are junctions and the rest file links, as the existing rows are. Update each script's header comment to say it links every v2 entry. Change no linking logic. Test only with `CLAUDE_CONFIG_DIR` pointing at an empty scratch directory; never run either script against the real config dir.
- **Acceptance**:
  - forward: `grep -q 'commands/plan-v2.md' scripts/link-v2-review.sh && grep -q 'commands/implement-v2.md' scripts/link-v2-review.sh && grep -q 'implement-v2-milestone.js' scripts/link-v2-review.ps1` (today: exit 1)
  - guard: `CLAUDE_CONFIG_DIR="$(mktemp -d)" sh scripts/link-v2-review.sh --dry-run` exits 0 (today: exit 0; the script exits 1 when it refuses any entry). It turns red if an entry is listed whose source tasks 12 to 17 have not created.

### 20. Describe the engine in CLAUDE.md [M]
- **Files**: `CLAUDE.md`
- **Depends on**: 17, 19
- **Action**: Replace the "cannot be run yet" sentence and add an "Implement v2" section.
- **Detail**: In "Planning v2", replace the sentence saying an approved v2 plan cannot be run with a pointer to `/implement-v2`. Add a `## Implement v2` section after it, in the style of "Planning v2": the carrier and skill, the two worker agents and their write rule, the Workflow, one gate and one commit per milestone, and a Deploy bullet. Change the "Review v2" Deploy bullet to say the link scripts cover every v2 entry, and the "Planning v2" Deploy bullet to point at those scripts instead of asking for a symlink per entry. In "Flow agent tiers", name `plan-worker-lite` and `plan-worker-deep` as following the bucket rules. In "Build discipline in multi-agent flows", add `/implement-v2` to the flows and `plan-worker-*` to the sub-agents the rule binds. Leave "Task store" unchanged; it lists no statuses. Add no history and no task or finding ids.
- **Acceptance**:
  - forward: `grep -q 'cannot be run yet' CLAUDE.md; test $? -eq 1` (today: exit 1)
  - forward: `grep -q '^## Implement v2' CLAUDE.md` (today: exit 1)

### 21. Say a parked closing task stalls its backlog item [S]
- **Files**: `claude/skills/tomlctl/references/backlog-reconcile.md`, `claude/commands/backlog.md`
- **Depends on**: 1
- **Action**: Add `parked` to the two statements of the `stalled` bucket rule.
- **Detail**: In `claude/skills/tomlctl/references/backlog-reconcile.md`, the `stalled` row of the bucket table says a closing task is `deferred` or `failed`; make it `deferred`, `failed` or `parked`. In `claude/commands/backlog.md`, the paragraph that opens with the `orphaned`, `stalled`, `dangling` buckets says the same of a `stalled` claim; make the same change. Change nothing else in either file.
- **Acceptance**: forward: `grep -q 'parked' claude/skills/tomlctl/references/backlog-reconcile.md && grep -q 'parked' claude/commands/backlog.md` (today: exit 1)

### 22. Retire the no-engine statements in the plan-v2 engine references [S]
- **Files**: `claude/skills/plan-v2-engine/references/approve.md`, `claude/skills/plan-v2-engine/references/write-and-prove.md`
- **Depends on**: 5, 17
- **Action**: Replace the two sentences that say no engine exists and that premises cannot be recorded.
- **Detail**: In `claude/skills/plan-v2-engine/references/approve.md`, the approval warning ends with the sentence "No engine can run it yet."; replace that sentence with one naming `/implement-v2` as the command that runs the flow, and keep the rest of the warning, since the v1 carriers still cannot run it. In `claude/skills/plan-v2-engine/references/write-and-prove.md`, replace the sentence saying there is no `--record` for premises with one saying `tomlctl plan probes --record <capture> --premises` stores a verdict per premise; write the command in a backtick span, not a bash fence. Change nothing else in either file.
- **Acceptance**:
  - forward: `grep -q 'No engine can run it yet' claude/skills/plan-v2-engine/references/approve.md; test $? -eq 1` (today: exit 1, the sentence is present)
  - forward: `grep -q 'There is no .--record. for premises' claude/skills/plan-v2-engine/references/write-and-prove.md; test $? -eq 1` (today: exit 1, the sentence is present)

## Dependency Graph

Per-task `Depends on` lines are authoritative; this section states only the checkpoint cuts.

— CHECKPOINT A after tasks 2, 6, 8, 9, 10, 11, 21 — dependency closure: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 21. The tomlctl and glimpse surface, complete, documented and versioned (installable increment)

— CHECKPOINT B after tasks 18, 20, 22 — dependency closure: 1, 3, 4, 5, 7, 9, 12, 13, 14, 15, 16, 17, 18, 19, 20, 22. The engine's agents, Workflow, skill and carrier, with the contracts and deploy scripts that make it usable

## Risks

- A worker and the orchestrator write the same row with no compare-and-set — the engine gives each row one writer at a time, and re-entry resets stale `in-progress` rows only when no run is open.
- The runtime restarts a stalled worker with its edits in place — the procedure's own-diff step is written for that case, and a worker that finds its row already `done` returns without editing.
- `SubagentStart` may not fire for Workflow agents — progress is read from task `status`, which workers write themselves; the correlation change in task 8 only helps when the hook does fire.
- Two documents sit close to their line ceilings (`plan.md` at 591 of 600, the task-store skill at 478 of 500) — task 10 moves the probes section out and task 18 caps its addition.
- No exhaustive `match` on `Status` exists outside task 1's files, so the compiler names nothing there — the sites that compare statuses are assigned by hand: the `stopped` test in `tomlctl/src/backlog/reconcile.rs` (task 1), `failed_tasks` in `tomlctl/src/plan/view.rs` (task 6 adds `parked_tasks`), and the `--status` help text (task 3).
- The premise `probe` key might be rejected or audited by spec-input validation — task 5 mirrors `Accept.probe` in `add.rs` and `update.rs` as well as `schema.rs`, and its tests write a spec through `tasks add-many` after a record.
- The premise `probe` key carries no schema stamp of its own, so an older binary drops recorded verdicts on its next write — the engine re-runs premises at every launch, so only the record and the worker view are lost, and After Merge reinstalls the binary.
- `human-only-handovers` replaces `plan handover add` and rewrites the handover contract in `docs/ideas/v2-pipeline.md` — the engine references name the handover verb in prose only, and After Merge lists the amendment that payload needs before its first run.
- A stop leaves a milestone's own edits uncommitted — the pre-flight exempts the files of non-`pending` rows with an empty `commit`, and re-entry never abandons a run that may still be live.
- The engine prose cannot be exercised by any test — the lint gates check its commands and names, and the first real run on `human-only-handovers` is listed under After Merge as its proof.
- The plan is over the 25-file line — two checkpoint groups keep each gate's footprint coherent, and group B has no Rust beyond one test tuple.

## After Merge

- Run `cargo install --path tomlctl` and reinstall glimpse.
- Run `sh scripts/link-v2-review.sh --dry-run`, then without the flag, to link the new entries into the Claude config dir; restart open sessions.
- Before the first run of `human-only-handovers`, amend that flow through audited verbs so its documentation task also covers `claude/skills/implement-v2-engine/` (the handover verb and record shape it changes) and its `docs/ideas/v2-pipeline.md` task is reconciled with Approach, "Handovers".
- Run the two **predicted, unverified** success criteria by hand: the `tasks ready --checkpoint A` check, then `/implement-v2 human-only-handovers`.
- On that first run, check whether `agents.toml` gains a row for a Workflow worker, which settles the hook question in Research Notes.

## Exploration Notes

**Carrier and engine pattern** (`claude/commands/plan-v2.md`, `claude/skills/plan-v2-engine/`)
- Carrier: frontmatter `description` + `argument-hint`; `## Bindings` table; `## Pre-flight` (invoke `tomlctl` skill, `tomlctl capabilities --get subcommands|features`, stop if `AskUserQuestion` or `Workflow` absent); `## Run` hands off to the engine skill. No flow envelope.
- Engine skill: role rules, `## Rules`, `## Arguments`, `## Steps` table (Step | Run step | Skipped when | Reference), `## Re-entry`, references under `references/`.
- Launch sequence: args to `<scratch>/<step>-args.json`; `Workflow({name, args})`; `tomlctl plan run start --slug --step --workflow --args-hash --run-id --session`; end the turn; on notification `plan run finish --step --outcome ok|errors|stopped`. An index ending `no agent launched` is closed as `errors` and never relaunched with the same args.
- `disableWorkflows` has no engine handling; the carrier pre-flight is the only stop.

**Workflow scripts** (`claude/workflows/plan-v2-{investigate,prove,review}.js`, `review-v2.js`)
- `export const meta = { name, description, whenToUse, phases }`; `SCRIPT_VERSION`; `argProblems()` then `fail('no agent launched')`; `caught()` wrapper; `agent(prompt, { agentType, schema, label, phase })`; return an index `{slug, entries, errors, script_version}`. Return schemas use `additionalProperties: false`.
- Prompts are built from `args` alone. Agents write their own outputs.

**Gates a new entry must pass** (`tomlctl/src/cli/dispatch/tests/{lint.rs,skills.rs}`)
- `command_lint`, `guidance_lint`, `flag_table_lint` over bash fences in commands, agents, skills and backtick spans in workflow scripts.
- `workflow_scripts_name_real_agents`: literal `meta.name`, not a command stem; every `agent()` has a literal `agentType` naming a real agent; no `model`/`effort`.
- `md_workflow_launches_name_saved_scripts`: each `Workflow({name: "x"` in markdown matches a script's `meta.name`.
- `carrier_invokes_required_skills`: add a tuple to `expected` in `skills.rs`.
- Skill size ceilings: SKILL.md 500 lines, references 600, description 1024 chars.

**tomlctl surface that exists**
- `plan view --slug S --for worker --task N`: markdown only; intent, non-goals, binding decisions, contracts, row, spec (outcome, constraints, accept, premises, means, executor), design sections. No gates, no handovers.
- `tasks ready --slug S [--in-flight ids]`: `{ready, held, next, blocked}`; no milestone scope.
- Milestone membership is a task's `checkpoint`; `[[milestones]]` holds `gates[]`; `tasks closure --checkpoint A` lists members; `tasks train --checkpoint A` groups done uncommitted rows.
- `plan probes --emit --premises [--milestone A]`: prints a bash script over pending rows; `at = "base"` premises only; `after:<id>` premises are skipped; no `--record` for premises.
- `plan drift`: `{approved_commit, changed_files, affected}`; a commit is the plan's own only when a row's `commit` holds it.
- `plan stage`: post-approval stages `approved | running | done` from row statuses.
- Audited after approval: `tasks add-many`, `update --ndjson`, `remove`, `plan milestone` need `reason` and write `[[changes]]`. `status`, `agent`, `commit` are unaudited: `tasks update N --slug S --status in-progress --agent X` works on an audited store.
- Writes take an exclusive file lock with a 30s retry; concurrent workers queue.
- Task statuses: `pending | in-progress | done | failed | deferred`. No parked or `needs_human` status.
- `plan handover add|done` and `plan run start|finish|abandon|skip` are unaudited writes to `plan.toml`.

**Workers** (`claude/agents/implement-{deep,lite}.md`)
- Shared blocks (`scripts/shared-blocks.toml`): `forbidden-working-tree-ops`, `file-edits`, `backlog-candidates`, `implementer-verification`, `implementer-report-lines` (both implementers only); `tomlctl-for-agents` (also research and investigator agents).
- `forbidden-working-tree-ops` forbids `tomlctl tasks update`; a "Commit Discipline" section allows instructed commits. Both conflict with the engine decisions.
- Vocabulary is v1: `Action`, `Acceptance`, cluster `files[]`, ledger ids, `FILE-BUDGET`, fetch by `tasks show`. Report tags: `applied`, `skipped`, `escalate`, `deviation:`, `TANGENTIAL:`.
- No pre-start check of the worker's own diff.
- `verification` agent: input `commands:` (with `timeout:`, `rerun:`) and `transient:`; output per command `outcome pass|fail|timeout|flaky`.
- `/implement` gate pattern: drain, `verification`, `tasks train`, selective staging, `tasks update --commit <sha>`; at most 2 fix cycles.

**Contract conflicts to amend**
- `claude/skills/flow-contract-task-store/SKILL.md` §11 "Status writes are the orchestrator's" and `docs/plans/v2-planning.md` ("§11's one-writer rule stands") contradict workers writing their own `status`, `agent`, `commit`.
- `CLAUDE.md` "Planning v2" says an approved v2 plan cannot be run.

**glimpse**
- Reads `status`/`agent` from `tasks.toml` by revision. Agent-to-task correlation matches the literal `tasks show <ids> --slug <slug>` in the dispatch prompt (`tomlctl/src/agents/correlate.rs`); a `plan view` prompt does not link.

**First payload: `human-only-handovers`**
- 12 pending tasks; milestone A = 1-7, B = 8-12; task 11 (B) needs 1 and 3 (A). Deep: 1, 8, 9; lite: the rest.
- `tasks ready` returns `[1, 8, 12]`, mixing milestones.
- Gates: A = nextest, fmt, two clippy; B = a unit test plus `cargo install --path tomlctl` and `--path glimpse`.
- `plan check` ok; `plan stage` = `approved`; `plan drift` flags 9 of 12 tasks because no row records a commit since approval at 9890f47.

**Gaps needing tomlctl code**
1. Milestone scope for `tasks ready`.
2. `after:<id>` premises at dispatch time; a per-task scope for premise probes.
3. A parked state for a task or milestone, and a place for worker deviations and handoff notes.
4. Drift usable mid-run.
5. Gates and handovers are absent from the worker view.
6. glimpse correlation for a `plan view` prompt.

**Deploy**
- `scripts/link-v2-review.{sh,ps1}` list only review-v2 entries; plan-v2 entries are linked by hand.

**Backlog**
- B-735ed4dd (open, `scripts`): neither link script covers the plan-v2 carrier, engine skill, agents and Workflows.
- B-c5783e08 (open, `claude/skills/flow-contract-vet-research`): unrelated to this change.
- B-55a13775 (open, `claude/skills/review-v2-method`): unrelated to this change.

Scope check: about 20 unique files expected, under the 25-file split line; a checkpoint-cadence question applies.

## Research Notes

No dependency is added or upgraded. Vetted 2026-11-09; no `[[vet_events]]` row was written because no flow ledger exists before Phase 9.

**Workflow script runtime** (high; `workflow-authoring` skill, https://code.claude.com/docs/en/workflows.md)
- `agent(prompt, {label, phase, schema, isolation, agentType, stallMs})`, `parallel(thunks)`, `pipeline(items, ...stages)`, `phase()`, `log()`, `workflow(name, args)` (one level). Up to 16 agents run at once.
- With `schema`, `agent()` returns the validated object and throws after 5 failed validations; a throw inside `parallel()` becomes `null`.
- Plain JS loops with `await` are allowed, so wave N+1 can be computed from wave N's returns. The script must be deterministic: no clock, no randomness, no filesystem, no shell. Prompts must depend only on `args` and earlier returns.
- Limits: 1,000 agents per run, 4,096 items per `parallel()`. No documented script timeout or args/return size limit.
- A stalled agent restarts from the same prompt without its transcript, up to 5 times; its file edits stay. An awaited agent that fails all attempts ends the run; inside `parallel()` its slot is `null`.
- `resumeFromRunId` returns cached results for completed agents and reruns the first failed agent and every agent after it. Same session only.
- The launch returns `{status: 'async_launched', runId, error?}`; a script failing its syntax check still reports `async_launched`, so `error` must be read.
- Impact: wrap each worker call in `caught()`; keep the launch-then-end-turn sequence; writers never resume.
- Searched: the installed skill; code.claude.com `workflows.md` and the Agent SDK TypeScript reference, fetched 2026-11-09.

**Workflow agents share the session cwd** (medium, hypothesis)
- Inferred from the docs' cache rule. Verify: the first worker's pre-start step prints `git rev-parse --show-toplevel` and stops if it is not the path in its prompt.

**Hooks for Workflow agents** (low, hypothesis; https://code.claude.com/docs/en/hooks.md)
- The docs promise `SubagentStart` only for the Agent tool. Whether it fires for a script's `agent()` call is unknown, and no local hook log exists. Verify: one run with `glimpse hook` logging.
- Impact: live progress must not depend on hooks. Task `status` written by the worker is the progress signal.
- Searched: code.claude.com `hooks.md`, `workflows.md`, `sub-agents.md`, fetched 2026-11-09; no source settles it.

**glimpse correlation** (high; `tomlctl/src/agents/correlate.rs` `dispatch_in`)
- The task regex is `tasks show ([0-9]+(?:,[0-9]+)*) --slug (<slug>)`; a `plan view --for worker --task N` line does not match.
- Impact: either the worker prompt carries a `tomlctl tasks show <n> --slug <slug>` line, or `dispatch_in` gains a `plan view` pattern.

**Status-write rules** (high)
- `claude/skills/flow-contract-task-store/SKILL.md` §11 names `update` an orchestrator verb. Restated in the `forbidden-working-tree-ops` block of both implementers and in `docs/plans/v2-planning.md` (Cold readers, Deferred elaboration).
- `tasks update <id> --slug <slug> --status <s> --agent <a> --commit <sha>`: any transition is accepted, a repeat write succeeds with `changed: []`, and there is no `--expect-status`.
- Impact: two writers on one row overwrite silently, so the engine gives each row one writer at a time: the worker while it runs, the orchestrator between runs.

**Permissions** (high; `.claude/settings.json`, permission-modes docs)
- `Bash(tomlctl *)` is allowed to subagents. A Write-tool write under `.claude/flows/**` is a protected path; `docs/flows/**` is not.
- Impact: workers write notes under `docs/flows/<slug>/notes/` and reach `.claude/` only through tomlctl.
