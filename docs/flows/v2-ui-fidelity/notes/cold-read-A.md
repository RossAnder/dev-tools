Verdict: a worker could carry out most of milestone A as written. Six points would leave a worker guessing or stuck (tasks 1, 4, 5, 17, 27 and 28), and two claims about the code are false (tasks 11 and 28). Every other task is actionable as written.

```toml sources
read_at = "52fe01b9a17c76c7068e241fa891e6cacd6fcbc6"
dirty = []

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 556
claim = "from_toml refuses any schema_version above SCHEMA_VERSION (3), so a store stamped SCHEMA_VERSION + 1 is refused by the same binary"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 516
claim = "write_version has two tiers today, BASE_SCHEMA_VERSION (2) and SCHEMA_VERSION (3)"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 1840
claim = "the test newer_schema_version_is_refused asserts that SCHEMA_VERSION + 1 is refused"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 706
claim = "a test helper builds MilestoneSpec with a struct literal and no Default, outside task 4's files"

[[source]]
path = "tomlctl/src/tasks/milestones.rs"
line = 294
claim = "a test helper builds MilestoneSpec with a struct literal, outside task 4's files"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 368
claim = "MilestoneSpec derives Debug, Clone, PartialEq and Eq but not Default"

[[source]]
path = "tomlctl/src/plan/store.rs"
line = 714
claim = "a test helper builds Accept with a struct literal, outside task 5's files"

[[source]]
path = "tomlctl/src/plan/run_gate.rs"
line = 141
claim = "target_round keys gate rounds by milestone and round alone, and --rerun merges into the latest seq of that key"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 362
claim = "the gate step records its rounds with plan run gate --round <n> and re-runs with --rerun"

[[source]]
path = "tomlctl/src/review/catalogue.rs"
line = 841
claim = "the catalogue validator errors when a (family, tag) does not have exactly one owner, so zero owners is also an error"

[[source]]
path = "claude/skills/review-v2-lenses/vocabulary.toml"
line = 238
claim = "the vocabulary comment states that the validator needs exactly one owning lens per (family, tag)"

[[source]]
path = "claude/skills/review-v2-lenses/checks/lint-baseline.md"
line = 1
claim = "a verification check resolves a fixed command from its discover ladder, and the surveyor runs nothing"

[[source]]
path = "claude/skills/review-v2-lenses/SKILL.md"
line = 101
claim = "the Check entry-format table is at lines 101-112, while line 189 is the project layer's [[checks]] row"

[[source]]
path = "claude/commands/plan-v2.md"
line = 34
claim = "plan-v2.md's pre-flight step 2 names only review_apply and milestone_schedule, not accept_run_by"

[[source]]
path = "tomlctl/src/cli/types/mod.rs"
line = 208
claim = "FEATURES holds accept_run_by at line 208, confirming task 11's anchor"

[[source]]
path = "claude/workflows/tests/harness.test.mjs"
line = 28
claim = "the harness builds its AsyncFunction loader inline and exports nothing"

[[source]]
path = "tomlctl/src/plan/view.rs"
line = 644
claim = "the Standing line is rendered here; view.rs already contains the word class three times"

[[source]]
path = "tomlctl/src/plan/approve.rs"
line = 66
claim = "approve.rs already contains a one_way binding in gate_report"

[[source]]
path = "claude/skills/plan-v2-engine/references/approve.md"
line = 135
claim = "approve.md already contains one-way and class before task 24 edits it"
```

## Unactionable

1. **Task 1, Detail: "extend write_version (schema.rs:516) to stamp SCHEMA_VERSION + 1 when any decision has a class".** At HEAD, `SCHEMA_VERSION` is 3, and `from_toml` refuses any store whose `schema_version` is above it (`tomlctl/src/plan/schema.rs:556`). The test `newer_schema_version_is_refused` (schema.rs:1840) asserts exactly that refusal. A worker who follows the sentence literally writes a store that its own binary then refuses to read. The plan never says what to do instead. The open choices are:
   - raising `SCHEMA_VERSION` to 4, and so the meaning of the constant;
   - which stamp a store gets when it holds classes but no run records;
   - whether a third tier sits between `BASE_SCHEMA_VERSION` and the current version.

   A worker has to design the version scheme alone.

2. **Task 4, Constraint: "Touch no file outside this task's files."** Adding `ui_states` and `ui_controls` to `MilestoneSpec` breaks two struct literals outside the task's files: `tomlctl/src/plan/lane.rs:706` and `tomlctl/src/tasks/milestones.rs:294`. Both are `#[cfg(test)]` helpers. `MilestoneSpec` has no `Default` derive (`tomlctl/src/tasks/schema.rs:368`), so the crate's tests will not compile unless those files are edited. The plan does not say whether those two files may be edited or whether the worker should add a `Default` derive instead.

3. **Task 5, the same Constraint.** Adding `states` to `Accept` breaks the struct literal in `tomlctl/src/plan/store.rs:714`, a test helper outside the task's files. The literal in `tasks/update.rs:531` uses `..clone()` and is unaffected. The plan leaves the same open question as task 4.

4. **Task 17, Detail: "record one gate row per state with command `visual <state>` ... through plan run gate".** `plan run gate` keys a round by `(milestone, round)` alone (`tomlctl/src/plan/run_gate.rs:141`). The task does not say which `--round` the visual pass passes. With the gate's own round number, the visual record becomes a later `seq` of the gate's round. The gate step's `--rerun` (drain.md:362) merges into "the round's latest recorded run", so a later gate rerun would merge into the visual record. Whatever reads the round's latest run would also see visual rows as the gate's outcome. The task also leaves `<run dir>` unplaced: it does not say whether captures go to scratch or under `docs/flows/<slug>/`. That matters because the user is later given "each state's capture path".

5. **Task 27, the concern tags of intent-fidelity.** The task adds `criteria-fidelity` and `decision-fidelity` to vocabulary.toml, but their owning lens arrives only in task 29. The validator errors on any `(family, tag)` without exactly one owner, zero included (`tomlctl/src/review/catalogue.rs:841`; vocabulary.toml:238). Until task 29 lands, the catalogue dispatch test fails. That test is task 27's own accept guard, and CLAUDE.md tells a worker to run it after a `claude/**/*.md` edit. Task 6 says outright that its analogous failure is expected until task 9; task 27 does not. A worker cannot tell whether to make the failure go away, for example by dropping the tags or adding a stub owner.

6. **Task 28, Detail: "Capture every state --list prints, capped at twelve".** A `verification` check's command is fixed by its `discover` ladder, and the surveyor "runs nothing" (`checks/lint-baseline.md`). The verification agent then runs that fixed command list. To cap at twelve, someone has to run `--list` first and build the `--only` flags from its output. The plan does not say who runs `--list`, or whether the check should drop the cap and capture every state with no `--only`. The mechanism is left to the worker.

## False premises

1. **Task 11, Detail: "accept_run_by is the precedent: it was added to the same places".** `accept_run_by` appears in `cli/types/mod.rs:208`, `tests/capabilities.rs:1856`, the README and `implement-v2.md:29`, but not in `claude/commands/plan-v2.md`. That file's step 2 (line 34) names only `review_apply` and `milestone_schedule`. The task still names plan-v2.md step 2 directly, so a worker is not blocked.
2. **Task 28, Detail: "SKILL.md's check entry format (around line 189)".** Line 189 of `claude/skills/review-v2-lenses/SKILL.md` is the project layer's `[[checks]]` row. The Check entry-format table is at lines 101-112.

## Premises that hold (probed)

All the premise commands the tasks carry exit 0 at HEAD:
- task 1: `fn decision_to_toml`
- task 2: `fn unanswered_decisions`
- task 4: `pub(crate) gate_probes`
- task 5: `RUN_BY_OWNERS`

These cited locations match the code, give or take a few lines:
- task 1: schema.rs:16, 516 and 724, and records.rs:27; the projection is `record_value`, records.rs:454
- task 2: store.rs:350, approve.rs:58, 161 and 226, and stage.rs:246 and 273
- task 3: view.rs:383 and 644
- task 4: tasks/schema.rs:369 and 1317-1387, and cli/types/plan.rs:435 and 484
- task 5: tasks/schema.rs:49, 276, 309, 1176 and 1200, add.rs:99 and 624, and probes.rs:174-185 and 874
- task 6: check.rs:464, at `decision_finding`
- task 7: emit.rs:169 and 213
- task 8: inputs.rs:58
- task 16: the `workflow_scripts_logic_passes_under_node` gate, lint.rs:166
- task 19: SKILL.md:18, 26, 89 and 105, drain.md:367, 472 and 563, and run-milestone.md:17, 40 and 246
- task 21: SKILL.md:16, and open-and-challenge.md sections 1 and 5
- task 23: write-and-prove.md lines 82-88
- task 24: elaborate.md:51 and 220, and approve.md:138
- task 26: survey.rs:652
- task 29: compose.md:32

Task 15's conditional resolves to "no delivery block", because the `research-delivering` block's file list in `scripts/shared-blocks.toml` does not include every agent. `plan run start --step` accepts any string, so `visual:<id>` is fine. `inputs list` takes `--kind`, `--where` and `--get`, and `inputs answer` exists.

Not counted, but worth a worker's notice: task 16 says "reuse its loader", but `claude/workflows/tests/harness.test.mjs` exports nothing (line 28 builds the loader inline). The harness is outside task 16's files, so the loader has to be copied rather than imported.

## Check commands

Each worker-run check was run against the present tree; all should fail before the change.

- **Task 3:** `grep -q 'class' tomlctl/src/plan/view.rs` **passes now**. view.rs already contains "class" three times, so this check cannot discriminate.
- **Task 2:** the whole check fails now, which is correct. Its `approve.rs` half (`grep -qi 'one_way'`) already passes, so only the store.rs half discriminates.
- **Task 20:** the whole check fails now. Its `grep -q 'SendMessage' plan-worker-deep.md` half already passes, so it does not prove the `tools:` grant was added.
- **Task 21:** the whole check fails now. Its `grep -qi 'opens'` half already passes (three matches).
- **Task 23:** the whole check fails now. Its `open-and-challenge.md#5-surface` half already passes (one match).
- **Task 24:** the whole check fails now, but only on its elaborate.md halves. `grep -qi 'one-way' approve.md` and `grep -q 'class' approve.md` both pass today, so the check never sees task 24's approve.md edit.
- **Fail now as expected:**
  - tasks 1, 4, 5, 6, 7, 8, 9, 10, 11 and 12
  - tasks 13, 14, 15, 16, 17, 18, 19, 22, 25 and 26
  - tasks 27, 28, 29, 30 and 31

  Each is a grep or `test -f` that exits 1 on the present tree.
- **Not run:**
  - the gate-owned checks: every `cargo nextest` and `cargo test`, `node --test claude/workflows/tests/`, `bash scripts/verify-shared-blocks.sh` and `bash claude/skills/uicapture/scripts/render.sh --self-test`. These are whole-crate builds or need the change, so they are left to the orchestrator.
  - task 14's self-test, which also needs a Chromium-family browser in the gate environment; its presence was not checked.

## Coverage

I read the worker view and the files list for tasks 1 to 31. To check the claims, I read these:
- **Plan store code:** tomlctl/src/plan/{schema,records,store,approve,stage,view,emit,check,probes,run_gate,milestone,lane}.rs
- **Task store and inputs code:** tomlctl/src/tasks/{schema,add,update,milestones}.rs, tomlctl/src/inputs.rs and tomlctl/src/cli/types/{mod,plan}.rs
- **Review code:** tomlctl/src/review/{catalogue,survey}.rs and tomlctl/src/cli/dispatch/tests/lint.rs
- **Engine skills:** the implement-v2-engine SKILL.md, drain.md and run-milestone.md, and the plan-v2-engine SKILL.md and references
- **Review lenses:** the review-v2-lenses SKILL.md, vocabulary.toml, lenses and checks, and review-v2-engine compose.md
- **Carriers, scripts and the rest:** plan-v2.md, implement-v2.md, scripts/shared-blocks.toml, the link scripts, harness.test.mjs and the plan-worker frontmatter

I could not check:
- whether a Workflow-dispatched agent can SendMessage the main session (decision D9 rests on a probe from 2026-11-17 that I did not repeat);
- browser availability for task 14;
- every gate-run test.

I read none of the notes under notes/.
