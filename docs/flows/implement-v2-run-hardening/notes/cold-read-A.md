Verdict: a worker could carry out milestone A (tasks 1-10) as written; no point stops it (0 unactionable, 0 false premises), with two small readings noted below that a worker can settle from the text.

```toml sources
read_at = "1502e6b420be81caac7bc44ae0e369dc20e461c9"
dirty = []

[[source]]
path = "tomlctl/src/plan/drift.rs"
line = 174
claim = "uncommitted_files runs git status --porcelain=v1 -z --no-renames --untracked-files=all and skips .claude/flows/<slug>/ and docs/flows/<slug>/ through parse_status (line 205), as task 1 says"

[[source]]
path = "tomlctl/src/cli/types/plan.rs"
line = 88
claim = "PlanEmitOp holds LaunchArgs, GateList, ReproduceUnits, LaneCheck, Schedule; no CommitPaths yet"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 57
claim = "load_milestone_store exists; dispatch arms at lines 47-51 route lane-check to super::lane"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 285
claim = "Accept has run_by, covered_by, states; owner_problem at line 321; MilestoneSpec at 399 with gate_probes; SCHEMA_VERSION = 3 at 31; holds_v3_keys at 537; stamp chosen at 873"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 256
claim = "FindingLink carries a store path, which task 1's --for flow reads"

[[source]]
path = "tomlctl/src/plan/milestone.rs"
line = 84
claim = "gate_probes_for exists; the two MilestoneSpec literals task 3 owns are at lines 208 and 368; update records changes rows per field"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 745
claim = "a MilestoneSpec literal in a task 2 file; the other Accept and MilestoneSpec literals are in add.rs, update.rs, milestones.rs and schema.rs, all task 2 files"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 167
claim = "gate_list doc comment says holds are the orchestrator's and none is printed; output built at line 249 with visual added conditionally"

[[source]]
path = "tomlctl/tests/plan_emit.rs"
line = 648
claim = "gate-list tests seed stores from JSON, so task 4 can set external_gates without task 3's flag"

[[source]]
path = "tomlctl/src/plan/run_gate.rs"
line = 26
claim = "ROW_KEYS is command, outcome, exit, duration_s, summary, failed_ids, log; parse_row at 214; read_temp_log at 295"

[[source]]
path = "claude/agents/verification.md"
line = 151
claim = "the pass line and block formats task 6 parses match the task text, including failed_ids capped at 20 with a (+n more) tail and the .ids file beside the log"

[[source]]
path = "tomlctl/src/agents/audit.rs"
line = 15
claim = "DEFAULT_PATTERNS and compile (line 66); the two default-pattern unit tests at lines 170 and 191"

[[source]]
path = "tomlctl/src/cli/types/mod.rs"
line = 213
claim = "FEATURES holds plan_emit_lane_check"

[[source]]
path = "claude/commands/implement-v2.md"
line = 29
claim = "the pre-flight feature list and its refusal sentence that task 8 extends"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 332
claim = "step 7 says the verb holds nothing back; the hand-staged [[record]] block and mapping paragraph follow; step 10 at 436 uses plan drift --uncommitted and git status --porcelain --ignored; Stamping on re-entry at 481"

[[source]]
path = "claude/skills/tomlctl/references/plan.md"
line = 534
claim = "the plan emit pointer names only launch-args, gate-list and reproduce-units; check-class table holds plan/gate-probe-unknown at 565"

[[source]]
path = "claude/skills/tomlctl/references/plan-engine.md"
line = 180
claim = "gate-list section ends with the sentence that holds stay the orchestrator's"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 64
claim = "the gate-probe paragraph in section 1, the run_by bullet at 184 and the 4a findings paragraph at 228 that task 10 edits"
```

## Unactionable

None counted. Two readings a worker settles from the text, recorded so the orchestrator can tighten them if it wants:

- Task 3, "On `update`, `--gate` and `--external-gate` together replace the whole gate list and the whole mark list ... an update that passes neither keeps both." An update passing `--gate` alone is not named. Read with the "neither keeps both" sentence it means either flag replaces both lists, so `--gate` alone empties `external_gates` (and on an audited store appends an `external_gates` changes row). That reading also stops an orphan mark tripping task 5's `plan/external-gate-unknown`. It is the only sensible reading, but the constraint "A call without `--external-gate` behaves and prints as today" pulls slightly against it when marks already exist.
- Task 1, `--for flow`: "each review store a row's `[[finding_links]]` names". `finding_links` is a store-level table keyed by `ref` (`tomlctl/src/tasks/schema.rs:256`). The worker has to take "a row" to mean a row of this milestone, matched by `ref`. Picking all links instead only adds store paths that changed, so either choice is safe.

Task 2: raising `SCHEMA_VERSION` to 4 means the stamp at `tomlctl/src/tasks/schema.rs:873` (`if holds_v3_keys() { SCHEMA_VERSION }`) needs a separate v3 constant. Otherwise every v3 store would be stamped 4, which the constraint forbids. The detail ("a store holding neither keeps the stamp it has today") makes this clear enough, so it is not counted. No other crate file imports the tasks `SCHEMA_VERSION`.

Every Accept and MilestoneSpec struct literal is in task 2's files, except the two in `plan/milestone.rs`. Task 2 already says to request that file if needed. Grep found literals in `tasks/add.rs:619`, `tasks/update.rs`, `tasks/schema.rs`, `tasks/milestones.rs:294` and `plan/lane.rs:745`.

All ten tasks cite `design/direction.md#milestone-a`, which exists (`docs/flows/implement-v2-run-hardening/design/direction.md`, `## Milestone A`). No task is a UI task.

## False premises

None. Every stated premise holds at HEAD:

- `fn uncommitted_files` in drift.rs: PASS. The flags and skip list match the task 1 detail.
- `const SCHEMA_VERSION: i64 = 3`: PASS. `fn owner_problem`: PASS.
- `fn gate_probes_for`: PASS. `fn gate_list`: PASS. `plan/gate-probe-unknown` in check.rs: PASS.
- `ROW_KEYS` in run_gate.rs: PASS. `fn compile` in audit.rs: PASS.
- `plan_emit_lane_check` in both mod.rs and implement-v2.md: PASS.
- `^## 10. Commit` in drain.md: PASS. `holds nothing back`: PASS.

Other claims checked and found true:

- task 1:
  - `parse_status` and `normal_path` (schema.rs:714) exist.
  - `.git()` cases sit beside the emit leaves in tests/output_options.rs.
  - The plan-engine.md contents and section layout and the SKILL.md routing row (line 63) and reference blurb (line 88) all exist.
- task 2: `ACCEPT_FIELDS`, `accept_entry`, `accept_json` (show.rs:145), `entry_matches` (plan/store.rs:261), `accept_from_toml`/`accept_to_toml` and `milestone_from_toml`/`milestone_to_toml` all exist. The owners `worker|gate|covered|look` exist.
- task 3: `--clear-gate`, `--gate-probe` and the changes-row mechanism exist.
- task 4: the doc comment and the plan-engine.md sentence to replace exist, and the key-set test `["commands", "skipped"]` is at plan_emit.rs:721.
- task 6: the verification.md format, including `summary: none`, the `.ids` file and the `(+<n> more)` tail, matches the task. `ndjson` is currently a required field.
- task 9: the `[[record]]` block, `--rerun` second-dispatch text, step 10 recipe, `!!` handling and the paragraphs in apply-rows.md:223/274, questions.md:97 and run-milestone.md:35 are all present.
- task 10: the three anchor paragraphs exist.

## Check commands

Worker-run read-only accept checks, run on the present tree. Each fails now, as a forward check should before its change:

- `grep -q 'CommitPaths' tomlctl/src/cli/types/plan.rs`: FAIL
- `grep -q 'plan emit commit-paths' claude/skills/tomlctl/references/plan-engine.md`: FAIL
- `grep -q 'external_gates' tomlctl/src/tasks/schema.rs`: FAIL
- `grep -q '"effect"' tomlctl/src/tasks/add.rs`: FAIL
- `grep -q 'external_gates' claude/skills/tomlctl/references/tasks-store.md`: FAIL
- `grep -q 'external-gate' tomlctl/src/cli/types/plan.rs`: FAIL
- `grep -q -- '--external-gate' claude/skills/tomlctl/references/plan.md`: FAIL
- `grep -q '"held"' tomlctl/src/plan/emit.rs`: FAIL
- `grep -q 'plan/needs-gate-effect' tomlctl/src/plan/check.rs`: FAIL
- `grep -q 'plan/needs-gate-effect' claude/skills/tomlctl/references/plan.md`: FAIL
- `test -f tomlctl/src/plan/gate_blocks.rs`: FAIL
- `grep -q -- '--blocks' claude/skills/tomlctl/references/plan-run.md`: FAIL
- `grep -q 'scripts/' claude/skills/tomlctl/references/agents.md`: FAIL. It is weak, because any later mention of `scripts/` passes it, but it does discriminate today.
- `grep -q 'plan_emit_commit_paths' tomlctl/src/cli/types/mod.rs`: FAIL
- `grep -q 'effect_mark' claude/commands/implement-v2.md`: FAIL
- `grep -q '^version = "0.26.0"' tomlctl/Cargo.toml`: FAIL (it is 0.25.0)
- `grep -q 'plan emit commit-paths' .../implement-v2-engine/references/drain.md`: FAIL
- `grep -q -- '--blocks' .../drain.md`: FAIL
- `! grep -q 'holds nothing back' .../drain.md`: FAIL
- `grep -q -- '--external-gate' .../write-and-prove.md`: FAIL
- `grep -q 'plan/needs-gate-effect' .../write-and-prove.md`: FAIL

I did not run the gate-owned nextest checks, which need builds: `plan_commit_paths::`, `tasks_spec_add::|tasks::schema::`, `plan_milestone::`, `plan_emit::`, `plan_check::`, `plan_run_records::`, `agents::audit::`, `capabilities::` and `cli::dispatch::tests::`. Each is a narrow filter covered by the full-suite gate, and they are left to the orchestrator.

## Coverage

- Tasks read: 1-10, through the worker view and `tasks show --select id,files`.
- Files read or grepped:
  - Rust sources: plan/drift.rs, plan/emit.rs, plan/mod.rs, cli/types/plan.rs, cli/types/mod.rs, tasks/schema.rs, tasks/add.rs, plan/milestone.rs, plan/run_gate.rs, plan/check.rs and agents/audit.rs.
  - Tests: tests/output_options.rs, tests/it.rs, tests/plan_emit.rs and tests/capabilities.rs.
  - Skill and agent docs: references/plan.md, plan-engine.md and agents.md, tomlctl SKILL.md, claude/agents/verification.md, implement-v2.md, drain.md, apply-rows.md, questions.md, run-milestone.md and write-and-prove.md.
  - Crate and design files: tomlctl/Cargo.toml, Cargo.lock and design/direction.md (headings only).
- Not checked: compile behaviour of the struct-literal changes, and whether `cargo update -p tomlctl --offline` succeeds offline. Neither could be observed without a build or a write.
