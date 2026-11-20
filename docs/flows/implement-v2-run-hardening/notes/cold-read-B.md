Verdict: a worker could carry out milestone B as written except for two points: one false premise in task 11 (the base of the look index) and one unactionable point in task 14 (how index defects map onto the record's `defects`).

```toml sources
read_at = "1502e6b420be81caac7bc44ae0e369dc20e461c9"
dirty = []

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 204
claim = "gate-list numbers look entries with enumerate(), so visual.looks[].accept is 0-based"

[[source]]
path = "tomlctl/tests/plan_emit.rs"
line = 854
claim = "the gate-list test pins looks with accept 0 for a row's first entry and accept 2 for its third entry"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 73
claim = "the args example shows accept 1, which gives no ruling on the base"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 96
claim = "the index carries per state a note path and defects as {class, location, detail}"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 132
claim = "an unsure state the orchestrator settles as fail has no defect in the index"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 35
claim = "SCHEMA_VERSION 5 is both the newest stamp read and the request tier; from_toml refuses a stamp above it"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 548
claim = "write_version tiers the stamp on requests, decision classes and run records"

[[source]]
path = "tomlctl/src/cli/types/plan.rs"
line = 133
claim = "PlanRunOp holds Start..Report; a Visual leaf registers here"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 893
claim = "ui_scope_findings loads ui_capture::load_paths itself and emits the malformed-config warning"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 797
claim = "ui_findings takes only the store and emits plan/ui-accept-structural"

[[source]]
path = "tomlctl/src/ui_capture.rs"
line = 15
claim = "load_paths returns an empty GlobSet when the file or paths key is absent"

[[source]]
path = "tomlctl/tests/plan_check.rs"
line = 1290
claim = "write_capture_config helper exists beside the ui-accept-structural test"

[[source]]
path = "tomlctl/src/cli/types/mod.rs"
line = 218
claim = "FEATURES holds plan_run_records"

[[source]]
path = "claude/commands/implement-v2.md"
line = 29
claim = "the pre-flight feature list and the refusal sentence task 13 edits"

[[source]]
path = "claude/agents/plan-worker-deep.md"
line = 79
claim = "the shared procedure says a Run by: look entry is never yours to run"

[[source]]
path = "scripts/shared-blocks.toml"
line = 80
claim = "plan-worker-procedure and plan-worker-forbidden-ops are carried only by the two worker files"

[[source]]
path = "claude/skills/tomlctl/references/tasks-write.md"
line = 196
claim = "tasks update --ndjson rows take spec, so the mint can set spec.means after add-many"

[[source]]
path = "tomlctl/src/plan/view.rs"
line = 855
claim = "the worker view prints spec.means as a Means section, so no view change is needed"
```

## False premises

1. **Task 11, Detail:** "a `looks` entry that is not a `run_by = "look"` accept entry, by 1-based index as `gate-list`'s `visual.looks` prints it". gate-list prints a 0-based index. `tomlctl/src/plan/emit.rs:204` builds the index with `spec.accept.iter().enumerate()` and pushes `"accept": index` at line 219. The test `gate_list_visual_holds_states_controls_and_looks` (`tomlctl/tests/plan_emit.rs:854`) pins `{"task": 1, "accept": 0, ...}` for a row's first look entry. The two halves of the sentence contradict each other, so the worker has to pick one. If the worker follows "1-based", task 14's orchestrator prose has a problem: it copies look entries "from the index", which gate-list numbers from 0. The verb would then refuse a valid entry or match the wrong one. The visual.md args example (`"accept": 1`, visual.md:73) does not settle the base. The fix is to say "0-based, as gate-list prints it", or to state on purpose that the verb converts.

## Unactionable

1. **Task 14, Detail:** "...the failing states, their look entries, the defects from the index and the backlog id". Task 11 fixes the record's defect shape as `{state, class, location, note}`. The visual Workflow index (visual.md:96) carries `defects` as `{class, location, detail}` and a separate per-state `note`, which is the path of the inspector note. The plan leaves two things open:
   - Whether the record's `note` takes the note path or the inspector's `detail` text. The names match the path, but the path drops the defect description.
   - What to record for a state the orchestrator settled from `unsure` to `fail` (visual.md:132). The index has no defect for such a state. The defect is "what the note left unsettled", in prose the orchestrator wrote.

   The worker has to decide how each maps.

## Observations (not counted)

- Task 11: `SCHEMA_VERSION` (schema.rs:35) is both the request tier and the read ceiling (`from_toml` refuses `schema_version > SCHEMA_VERSION`, schema.rs:593). "A tier above the current highest" therefore means raising the read ceiling to 6 and keeping requests at 5 under a new name. The constraint "a store with no acceptance keeps its stamp" settles the behaviour, and the task's file list already includes schema_request.rs and store.rs, which reference `SCHEMA_VERSION`. A worker can derive this, though the plan does not say it.
- Task 11: the PlanStore literals in holders.rs:232, records.rs and store.rs:557 use `..PlanStore::default()` or `..written`. The ones that need filling are the full literals in schema.rs and schema_request.rs.
- Task 14 re-entry query: `tomlctl items list <plan.toml> --array visual_accept --where milestone=<id> --where round=<r> --get round` uses valid flags. On an existing plan.toml with `[[gate_round]]`, `--where milestone=A --where round=1 --get round` printed `1`, so `--where` matches an integer field. `backlog add` has `--kind` (accepts `debt`), `--flow` and `--context`.
- Task 15: both shared blocks it edits, `plan-worker-procedure` and `plan-worker-forbidden-ops`, are carried only by the two worker files (scripts/shared-blocks.toml:80-91), so a byte-identical edit in both keeps parity. Setting `spec.means` after `add-many` works through `tasks update --ndjson` with `spec` (tasks-write.md:196), and the worker view renders Means (view.rs:855).
- Task 12 is actionable as written. `findings` calls `ui_findings(&tasks)` and `ui_scope_findings(&tasks, &root)` back to back (check.rs:190-191), and these are their only callers.
- Task 13 is actionable. Every target named exists: FEATURES mod.rs:218, capabilities.rs:1670/1866, README.md:396/611, implement-v2.md:29.
- Several worker greps are weak, though they do discriminate today. `grep -q 'accepted'` (drain.md, report.md) and `grep -q 'ui-capture'` (worker files) would pass on any mention of the word.
- Direction: every task cites `design/direction.md#milestone-b`, and the specs follow its "inspector stays the only judge" reading. None is a UI-surface task, so no look choice is left open.

## Check commands

Worker-owned accept checks, run on the present tree (each failed with exit 1, so each one discriminates):
- `test -f tomlctl/src/plan/run_visual.rs`: FAIL(1)
- `grep -q 'visual_accept' tomlctl/src/plan/schema.rs`: FAIL(1)
- `grep -q 'plan run visual' claude/skills/tomlctl/references/plan-run.md && grep -q 'visual_accept' claude/skills/tomlctl/references/plan-store.md`: FAIL(1)
- `grep -q 'plan_run_visual' tomlctl/src/cli/types/mod.rs && grep -q 'plan_run_visual' claude/commands/implement-v2.md`: FAIL(1)
- `grep -q 'plan run visual' claude/skills/implement-v2-engine/references/visual.md`: FAIL(1)
- `grep -q 'accepted' claude/skills/implement-v2-engine/references/drain.md && grep -q 'visual_accept' claude/skills/implement-v2-engine/SKILL.md`: FAIL(1)
- `grep -q 'accepted' claude/skills/implement-v2-engine/references/report.md`: FAIL(1)
- `grep -q -- '-task-<n>' claude/skills/implement-v2-engine/references/visual.md`: FAIL(1)
- `grep -q 'ui-capture' claude/agents/plan-worker-lite.md && grep -q 'ui-capture' claude/agents/plan-worker-deep.md`: FAIL(1)
- `grep -qi 'concurrent' claude/skills/ui-capture/SKILL.md`: FAIL(1)
- `bash scripts/verify-shared-blocks.sh`: `shared-block parity: OK` (guard, passes today as expected)

Premise probes, all PASS at HEAD: `fn write_version` in schema.rs; `enum PlanRunOp` in cli/types/plan.rs; `fn ui_scope_findings` plus `fn load_paths`; `Two rounds at most` in visual.md; `is never yours to run` in plan-worker-deep.md; `plan-worker-procedure` in shared-blocks.toml.

Not run: the gate-owned nextest commands (`plan_run_records::|plan::schema`, `plan_check::`, `capabilities::`, `cli::dispatch::tests::`). They build the crate, which is left to the orchestrator's gate.

## Coverage

Read the worker views and file lists of tasks 11 to 15. Read or grepped: plan/schema.rs, plan/emit.rs, plan/check.rs, plan/holders.rs, plan/records.rs, plan/store.rs, plan/view.rs, plan/mod.rs, plan/dispatch.rs, cli/types/plan.rs, cli/types/mod.rs, ui_capture.rs, tasks/schema.rs (Means), tests/plan_emit.rs, tests/plan_check.rs, tests/capabilities.rs, tests/output_options.rs, README.md, implement-v2.md, implement-v2-engine SKILL.md and references visual.md, drain.md, report.md, both worker agent files, ui-capture SKILL.md, scripts/shared-blocks.toml, tomlctl references plan.md and tasks-write.md, and the `items list`, `tasks update` and `backlog add` help. I did not check the milestone A outputs that tasks 13 and 14 depend on (task 8's feature strings and drain.md's commit-paths paragraphs), since they do not exist yet.
