Verdict: restructure, narrowly: give each accept check an explicit owner (`run_by = worker | gate | covered`) apart from `writes_build`, and have the worker view, probes, `plan check`, `review schedule` and a new gate-list projection all read it. Every other item of the 30 extends the code in place.

# Structure challenge: v2 harness tuning

## Why the present structure does not hold for the accept-check items

Four items in the future state are about who runs an accept check:

- every accept check a worker sees is one it can run, and gate-only checks are marked as the gate's;
- the gate does not rerun an accept check the plan marks as covered by a gate;
- one check proves every fixed finding gone;
- the gate command list comes from one tomlctl call.

Today "who runs this check" has no field of its own. It is read from `writes_build`, which also decides two other things:

1. **Probe eligibility.** Only an explicit `writes_build = false` is probed (`tomlctl/src/plan/probes.rs:149-155`), and `plan check` warns `plan/probe-absent` on any pending `writes_build = false` entry that has no probe (`tomlctl/src/plan/check.rs:495-513`).
2. **Worker runs it.** The worker view prints `Writes build: yes|no` (`tomlctl/src/plan/view.rs:740-741`). Both worker agents run only the `no` checks and leave the `yes` ones "to the gate" (`claude/agents/plan-worker-lite.md:52`, `claude/agents/plan-worker-deep.md:52`).
3. **Gate runs it.** The drain gates every non-empty check whose `writes_build` is `true` (`claude/skills/implement-v2-engine/references/drain.md:237`).

**Claim 1. `Accept.writes_build` (`tomlctl/src/tasks/schema.rs:271-282`) forces a gate-only check to state something false, or to change three consumers at once.**
- `review schedule` gives each finding that is not a `remove` cleanup the check `review reproduce-status … --expect gone` with `writes_build` left out (`tomlctl/src/review/schedule.rs:721-728`). That field defaults to `true` (`tasks/schema.rs:1126-1129`).
- The check builds nothing. It reads `true` only so that workers skip it and the gate runs it, and `findings.md:160` says so ("with no `writes_build = false`, so the gate runs it rather than the worker").
- So the worker is shown "Writes build: yes" for a store read.
- Marking it gate-only by setting `writes_build = false` would break three things: probes would run it (`probes.rs:154`), `plan check` would warn `probe-absent` (`check.rs:495`), and the worker prose would tell the worker to run a check that only passes after the gate's reproduce step.
- What would refute this: a consumer of `writes_build` that already tells "builds" apart from "gate-owned". The grep over `tomlctl/src` and `claude/` finds none.

**Claim 2. A boolean cannot express "covered by a gate".**
- A covered check is one the plan says the milestone's full-suite gate already exercises. The worker does not run it, and the gate drops it from the command list.
- That is a third state beside "worker runs" and "gate runs". So the "do not rerun" item forces a new field however the work is planned.
- Adding it as a second boolean next to `writes_build` has a cost. Every `writes_build` site listed above would have to check both flags, and the pair (`writes_build = false`, gate-only) would still look probe-eligible to `probes.rs:154` unless that site is changed too.
- What would refute this: a gate-coverage marker that already exists somewhere in `Accept`, `MilestoneSpec` (`tasks/schema.rs:320`) or the drain. None exists.

**Claim 3. The gate list's one reproduce check must cover exactly the findings that the reproduce units cover.**
- The rule that assigns a finding fixed by two rows to the higher-id row's unit lives in prose today (`findings.md:76`). The gate side lives in other prose (`drain.md:231-247`).
- Once both become tomlctl output (launch args, reproduce units, gate list, each from one call), they read the same inputs: milestone members, `[[finding_links]].fixes`, and the current reproductions (`tomlctl/src/review/reproduce.rs:263`).
- What would refute this: a design in which the gate's reproduce check does not depend on the unit assignment, for example a store-wide `reproduce-status --flow --milestone` that ignores units. Then the shared-input argument for placing them together is weaker, and the field change still stands.

## The drafted structure

**Data.**
- `Accept` gains `run_by`, with three values: `worker`, `gate` or `covered`.
- An absent `run_by` is read from `writes_build` (`false` → `worker`, `true` → `gate`), so stored plans keep their meaning and audited stores need no migration.
- `writes_build` then keeps only its probe-safety meaning: probe a check only when `run_by = worker` and `writes_build = false`.
- `covered` may carry the gate command it is covered by, so the gate list can show the claim.

**Consumers that change, in one edit set:**
- `tasks/schema.rs`: `Accept`, its read at about line 1126 and its write at about line 1140.
- `tasks/add.rs:99` and `:599`: `ACCEPT_FIELDS` and the parse.
- `tasks/show.rs:152`.
- `plan/view.rs:729-753`: print `Run by:`, label gate-owned and covered checks as the gate's, and keep `Writes build`.
- `plan/probes.rs:149-155` and `plan/check.rs:495`: skip anything other than `worker`.
- The two worker agents' step 52: run the `worker` checks, never the others.
- `review/schedule.rs:709-729`: the reproduce check becomes `run_by = gate`. A worker-runnable check, such as the anchor `--gone` check where one exists, sits beside it, so that no fix task's only check is one the worker cannot run.
- `review/tier.rs:58`: `PROBE_FIX_KEYS` is an allowlist, so a probe fix cannot flip `run_by`. That is the right default, and nothing changes there.

**New read-only projections.**
- Three projections: milestone launch args, reproduce units, and the gate command list.
- Place them in one module under `tomlctl/src/plan/` (for example `plan/run_args.rs`), because of Claim 3.
- The gate list emits:
  - the milestone's `gates`;
  - every `run_by = gate` check, deduplicated as `drain.md:247` says;
  - one milestone-level reproduce check per linked store in place of one check per finding;
  - no `covered` check.
- Naming constraint: `plan gate` is already the approval screen (`tomlctl/src/cli/types/plan.rs:61-62`), so the gate-list verb cannot take that name. Use something like `plan milestone gate` or a `--for` role.

**What then stays prose.** In `drain.md` step 7 and `findings.md`, "Reproduce check", the hand assembly is replaced by the one call. The rule stays: a held command such as `cargo install` waits for a second dispatch.

## What extends in place (no restructure)

Each item below lands in one place that already exists. Nothing in the present layout forces an edit elsewhere.

**Verdict-return schema `allOf`.**
- Fix it once, at the source: `review/schema_out.rs:297-327`. Its top-level `allOf` (line 326) is used as the whole agent schema in three places: `claude/workflows/review-v2.js:220` (single verifier), `:226` (panel seat) and `claude/workflows/plan-v2-review.js:213` (reverify).
- A script-side fix would have to touch all three. Batches already nest it under `items` (`review-v2.js:175-179`).
- The tests `schema_out.rs:378-536` read `["allOf"]` and change with it. The unblocks↔fix_sketch pairing stays enforced by `review verdict`.

**Replayed `review add` duplicating an advisory.**
- `review/add.rs:142-149` supersedes only the cell's `Unverified` rows.
- An advisory minted `open` on a v2 flow (the `finish_row` comment, `add.rs:798`) survives a replay, and the replay mints it again. The fix is local to that filter.

**Fix-task files.**
- `review/schedule.rs:380-390` (`edited`) and `:608-618` (`row`) take only `edited_paths()`. Widening to files the fix names, test files included, changes those two functions and the `fold` grouping key (`:394-435`).

**Unreachable-claim warnings against done rows.**
- `tasks/check.rs:639-655` turns every `Graph::overlap_pairs` pair (`tasks/graph.rs:340-360`) into a warning. Filtering out pairs where either row is done and committed belongs in `overlap_findings`, not in the graph.

**`plan drift` own work.**
- `plan/drift.rs:216-228` keys own work on `Status`. Switching it to `agent ∈ {plan-worker-lite, plan-worker-deep}` is local. `affected_tasks` at `:247` and `:256` uses the same status test.

**C-quoted paths.**
- `plan drift` already reads `-z` (`drift.rs:174-212`).
- The quoting problem is in drain prose: `drain.md` step 6, "Edits follow their files", and step 10 run `git status --porcelain -- <paths>` without `-z`. It is a prose fix, or a call to the drift verb.

**Wave to slot scheduling.**
- `claude/workflows/implement-v2-milestone.js:188-250` is rewritten in place. Raw promise concurrency is already used by a saved script (`review-v2.js:241-259`). Argument validation, `toEntry` and the prompts survive.
- Two forced consequences for the planner:
  1. `siblings` (`:240`) becomes a snapshot taken at launch. A worker started earlier is never told the files of a task started after it. The worker prose (`plan-worker-*.md:56`) already treats any failure outside the task's own files as another worker's, so this is acceptable but should be stated.
  2. An `after:<n>` premise checked inside the launch cannot be recorded by the worker through `plan probes --record`. Premise verdicts are stored on the spec (`plan/view.rs:755-766`), and the drain counts every changed spec line as a forbidden worker write (`drain.md:93`). Either the in-launch check records nothing, or `drain.md` step 5 gains an exemption for premise `probe` lines.

**Worker return fields: files it needs, commands it ran.**
- Both are additions to `RETURN_SCHEMA` (`implement-v2-milestone.js:30-42`, `additionalProperties: false`, so `required` changes too) and to `entry` and `toEntry` (`:148-177`).

**Reproduce grep-decidable routing, batched plan verification, `run_dir` per relaunch.**
- The reproduce mode is a self-contained early-return block (`review-v2.js:38-145`). Routing units to a cheaper agent or a batch is added inside it.
- Batching a cell's singles for plan review is a pipeline arg read in `routeCell` (`:277-316`), passed by `plan-v2-review.js:326-336`.

**Script tests.**
- No extraction is needed. `review-v2.js` runs unchanged under stubbed globals: the source is wrapped in an `AsyncFunction(args, agent, parallel, pipeline, phase, log, workflow)` with `export` stripped. Run in the session scratchpad on 2026-11-11, the pipeline mode returned its full index with the expected `cell C1` and `verify F1` launches.
- One placement constraint: the test file must not be a top-level `*.js` in `claude/workflows/`. `command_lint_workflow_scripts` (`tomlctl/src/cli/dispatch/tests/lint.rs:151-160`) collects every such file, and `workflow_scripts_name_real_agents` requires each to open with `export const meta`.
- The hook already runs `cli::dispatch::tests` when a `claude/workflows/*.js` file is staged (`.githooks/pre-commit`, last gate). A Rust test that spawns `node` on a harness kept outside that directory gets run by the hook with no hook change. Whether `node` is guaranteed on every machine that runs the hook is the planner's call.
- The stubs' semantics for `parallel` and `pipeline` are assumptions about the runtime, and the tests should state them.

**The rest are local to their module:**
- `--template` unknown field: `tomlctl/src/output/template.rs`.
- Digest size: `review/digest.rs`.
- Field caps checked before the write: `review/fields.rs`.
- Hand-fixed disposition: the `FindingStatus` enum in `review/fields.rs`, plus the recheck targets.
- Advisory-tier plan findings that name an edit: the tier order in `review/tier.rs`, header at lines 1-14.
- Hook cost in a commit train: `.githooks/pre-commit`.
- The apply-v2-engine role rule: `claude/skills/apply-v2-engine/SKILL.md`.
- Gate pre-flight: run-milestone prose, or the gate-list projection run before the launch.

## Alternatives ruled out

- **Extract a shared JS module from the Workflow scripts for testability.** The scripts run unchanged under the harness, as above. The helpers repeated across scripts (`matches` at `review-v2.js:45`, `plan-v2-review.js:53` and `implement-v2-milestone.js:51`; `caught` at `plan-v2-review.js:95` and `implement-v2-milestone.js:180`) suggest the runtime offers no imports. That is an inference I did not check against the runtime.
- **Split reproduce mode out of `review-v2.js`.** It shares only `SCRIPT_VERSION` and `fail` with the pipeline mode, and a split renames the launch that `findings.md:101` and `plan run start --workflow` use. Nothing forces it.
- **Keep `writes_build` as the owner and add a `covered` boolean.** It still forces an edit at every `writes_build` site (Claim 2), and it leaves "Writes build: yes" on store reads (Claim 1).
- **Scatter the three one-call projections across `tasks/`, `review/` and `plan/`, next to `closure`, `reproduce-status` and `view`.** It works, but it splits the unit-assignment rule that the gate list depends on (Claim 3). I prefer the single module, but if the gate's reproduce check ignores units, nothing forces it.

## Coverage

- **Read in full:**
  - `claude/workflows/review-v2.js`
  - `claude/workflows/plan-v2-review.js`
  - `claude/workflows/implement-v2-milestone.js`
  - `tomlctl/src/review/schedule.rs`
  - `tomlctl/src/review/reproduce.rs`
  - `claude/skills/implement-v2-engine/references/run-milestone.md`
  - `claude/skills/implement-v2-engine/references/findings.md`
  - `.githooks/pre-commit`
- **Read in part:**
  - `tomlctl/src/review/add.rs` (87-206)
  - `tomlctl/src/review/schema_out.rs` (290-329, test names)
  - `tomlctl/src/plan/view.rs` (outline, 729-781)
  - `tomlctl/src/plan/drift.rs` (160-280)
  - `tomlctl/src/plan/check.rs` (480-530)
  - `tomlctl/src/tasks/check.rs` (580-690)
  - `tomlctl/src/tasks/graph.rs` (`overlap_pairs`)
  - `tomlctl/src/tasks/schema.rs` (`Accept`)
  - `tomlctl/src/review/tier.rs` (header, constants)
  - `tomlctl/src/cli/types/plan.rs` (verb list)
  - `tomlctl/src/cli/dispatch/tests/lint.rs` (150-160, 1030-1130)
  - `claude/skills/implement-v2-engine/references/drain.md` (gate section, porcelain lines)
  - worker agents (grep)
- **Not read:** `review/digest.rs`, `review/fields.rs` beyond its enums, `review/apply.rs`, `output/template.rs`, `apply-v2-engine/SKILL.md`, and the plan-review agents. The items that land there are judged local on the strength of their module boundaries alone.
