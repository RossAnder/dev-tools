Verdict: milestone A cannot be carried out as written: 1 unactionable point and 3 false premises stop it, two of them gate-breaking (task 2's compile scope, the finding-class scan for tasks 11, 12 and 14). The other 16 tasks are actionable.

```toml sources
read_at = "51fd3b2b88fa092fea7bfc510421e2a6c78f72b7"
dirty = []

[[source]]
path = "tomlctl/src/plan/records.rs"
line = 64
claim = "review_keys, record_edit (:403, :439) and record_value (:466) match RecordKind exhaustively, with no wildcard arm"

[[source]]
path = "tomlctl/src/plan/target.rs"
line = 143
claim = "target parsing (:143) and kind_name (:284) match RecordKind exhaustively, with no wildcard arm"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 17
claim = "module doc: unknown keys are dropped, because the store is tool-owned"

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 1935
claim = "test unknown_keys_are_dropped pins dropping an unknown key on a plan record"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/finding_classes.rs"
line = 47
claim = "source_bodies reads only the top-level *.rs entries of one directory, with no recursion"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/finding_classes.rs"
line = 404
claim = "plan_dir is src/plan, so src/plan/check/*.rs is never scanned"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/finding_classes.rs"
line = 382
claim = "a documented class missing from the scanned source is reported as documented but never emitted"

[[source]]
path = "tomlctl/src/io.rs"
line = 1899
claim = "repo_or_cwd_root lives at crate::io, not at the crate root"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 22
claim = "emit.rs already imports crate::io::repo_or_cwd_root"

[[source]]
path = "tomlctl/src/plan/store.rs"
line = 401
claim = "open_runs exists, pub(super), so plan::request can reach it"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 423
claim = "out_of_lane builds the lane as task 8 describes"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 482
claim = "section 11 runs from line 482 to 566 of a 594-line file, so task 15's move frees room for task 17"
```

## Unactionable

**U1. Task 2: adding `RecordKind::Request` breaks the build in files the task may not touch.** The Detail says "schema.rs gains only the PlanStore field, ..., RecordKind::Request with prefix "Q" in prefix() and mint_id", and the Constraints say "Touch no file outside this task's files" (schema.rs, schema_request.rs, plan/mod.rs). `RecordKind` is matched exhaustively, with no wildcard arm, in `tomlctl/src/plan/records.rs:64` (review_keys), `:403` (record_edit), `:439` and `:466` (record_value), and in `tomlctl/src/plan/target.rs:143` and `:284` (kind_name). A new variant is a compile error in all six places. The worker has to choose, and the plan does not settle it, between:
- editing records.rs and target.rs, which breaches the file list;
- adding wildcard arms, which also breaches it;
- minting Q ids outside `RecordKind`, which contradicts the Detail.

Ask the planner to either add records.rs and target.rs to task 2's files and say what a request does in each arm (refuse a review edit, for example), or replace the RecordKind route with a separate minting path.

## False premises

**F1. Task 2, Constraint: "Unknown keys inside a request, a path item or a change item are refused with the key named, as other plan records refuse them."** Other plan records do not refuse unknown keys; they drop them. The module doc at `tomlctl/src/plan/schema.rs:17-18` says "Unknown keys are dropped: the store is tool-owned and every write re-emits it from this shape". The test `unknown_keys_are_dropped` at `schema.rs:1935` pins that behaviour on a decision record. The constraint therefore asks for the opposite of its precedent. The worker cannot tell which half is meant: refusing, against the pattern, or dropping, against the constraint's wording.

**F2. Tasks 11, 12 and 14: the classes declared in `plan/check/` child modules are invisible to `documented_plan_classes_match_the_source`.** Task 14's Detail says the plan.md rows must match "the source exactly (documented_plan_classes_match_the_source compares them)", and its accept guard relies on that test. The test reads `source_bodies(&plan_dir())` (`tomlctl/src/cli/dispatch/tests/finding_classes.rs:47-55`). That function lists only the top-level `*.rs` entries of `src/plan` (`plan_dir`, `:404-408`) and never recurses. Tasks 11 and 12 put `plan/file-case-collision` and `plan/generated-output-unheld` in `tomlctl/src/plan/check/case_collision.rs` and `.../check/generated_outputs.rs`, which the test never scans. Once task 14 adds the two table rows, `disagreements` (`:382-385`) reports "`plan/...` is documented but never emitted", and `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests` fails at the gate. No task's files include finding_classes.rs. A fix needs one of:
- the scan recurses into `src/plan/check/`, with finding_classes.rs added to a task;
- the class string literals stay in check.rs;
- the two child modules move to the top level of `src/plan/`.

**F3. Task 9, Detail: "resolve the repo root with crate::repo_or_cwd_root".** The function is `crate::io::repo_or_cwd_root` (`tomlctl/src/io.rs:1899`), and emit.rs already imports it (`tomlctl/src/plan/emit.rs:22`). lib.rs has no crate-root re-export. This one costs little: a worker finds the right path at once. It is still a wrong claim about the code.

## Observations (not counted)

- D3's heading, bound to tasks 2, 4, 8 and 16, names the verb `plan run request`. Task 4's Action and Detail, the direction and every later task use `plan request add|rule|list|applied`. The task text settles it, but a worker reading D3 first may wonder. The heading could be reworded.
- Task 14 adds a pointer to `plan-request.md` in plan.md, and that file is created by task 13. Task 14's needs (1, 8, 9, 10, 11, 12) do not include 13. If task 14 runs before 13 and its worker runs the dispatch subset, as CLAUDE.md asks after a `claude/**/*.md` edit, `skill_markdown_links_resolve` fails on a missing target. At the gate both exist. Adding 13 to task 14's needs would remove the race.
- Task 4 has no test in its own files. Its accept filter `^plan_request::` is satisfied only by task 5's module. That is allowed by the milestone rule ("or in the task its needs names"), and the check is run by the gate, so it is not a blocker.
- Task 3 says "every row of the requester's checkpoint", while D6 says "milestone". Rows carry `checkpoint`, and emit.rs filters `row.checkpoint == milestone`, so the two coincide in v2.
- Each task's design field cites `design/direction.md#milestone-a`, and some also cite `#overarching`. Every spec follows the direction's strands. No task is a UI task.

## Premises checked and holding

- **Task 1.** `ui_capture.rs:10` `CONFIG`, `:15` `load_paths`, `:18` NotFound reads as empty, `:34-35` `./` trim and `literal_separator`. `mod ui_capture;` is at lib.rs:42 and `normal_path` at tasks/schema.rs:714.
- **Task 2.** `SCHEMA_VERSION = 4` is at schema.rs:33, the const asserts at :42-43, `vocabulary!` at :55, `ReportKind` at :189-202, `RecordKind`, `prefix` and `mint_id` at :207-220 and :550, and `write_version` at :537. `needs_person` is per entry (`run_report.rs:105`), so `Request` needs no per-kind mapping. store.rs:609 and :634 use `SCHEMA_VERSION + 1` as "newer than newest", which stays correct at 5.
- **Task 3.** Graph has `build`, `closure_up` and `closure_down` (graph.rs:194, :260, :266), `nodes_of` is at graph.rs:78 and the precedent is tasks/closure.rs:34-35.
- **Task 4.** `open_runs` is at plan/store.rs:401 (pub(super), reachable from plan::request) and `Run.tasks` at schema.rs:355. The `PlanOp::Handover` wiring the task copies is at cli/types/plan.rs:31 and plan/dispatch.rs:18. The global `--where` and `--get` options exist.
- **Task 5.** `plan run start --tasks` exists, and it.rs registers `plan_*` modules in sorted order.
- **Task 6.** tomlctl/Cargo.toml:3 and Cargo.lock:1979-1980 are at 0.24.0.
- **Task 7.** `FEATURES` is at cli/types/mod.rs:40, capabilities.rs pins the list (~:1823), README has the feature table (:603) and sample block (:338), and implement-v2.md step 2 holds the backticked list (:29).
- **Task 8.** lane.rs:349 `lane_check` loads plan.toml (:357), and `out_of_lane` (:423) and `exempt` (:435) are as described.
- **Task 9.** `gate_list` is at emit.rs:172 with `listed` at :181, and the test `gate_list_orders_gates_then_gate_owned_checks` is at plan_emit.rs:671.
- **Task 10.** `worker` is at view.rs:489 and `file_lines` at :723.
- **Task 11.** `Store::file_note` is at tasks/schema.rs:574 and `FileKind::of_note` at :222. `findings()` is at check.rs:148, with `ui_scope_findings` at :187 and the approvals block at :188. The header class list is at check.rs:5-8 and `assert_reports_warning` at plan_check.rs:184.
- **Task 12.** The ui-capture fixture writer is at plan_check.rs:1293, and the malformed-config warning pattern at check.rs:887-897.
- **Tasks 15 and 17.** drain.md has 594 lines; section 11 runs :482-566 and `## Parked rows at the next invocation` starts at :567. `rg "step 11"` finds SKILL.md:63, drain.md:92, :113 and :462, and questions.md:33, and nothing outside the skill. The reference ceiling is 600 (skills.rs:933).
- **Task 16.** run-milestone.md's clean-tree list is at :32-36 and the QUESTION routing at :252. One task per row is enforced at update.rs:232. `tasks show --with spec` is a valid value.
- **Task 18.** Both blocks are carried by exactly the two worker files (shared-blocks.toml:80-91). The anchors are **Asking the user** (:53), its until-loop (:59), step 7 (:32), lite's "When to return failed" (:131), deep's "Judgement" (:131) and "One row and one question" (lite :149, deep :145).
- **Task 19.** apply-rows.md:225 "Failure on an apply row" and "apply failure <n>" (:237) exist. flow-contract-task-store/SKILL.md:436 holds the v2 worker exception, at 481 lines against a body ceiling of 500. questions.md has "The writes during a live run" (:90) and step 2 (:37).
- **Task 20.** write-and-prove.md has "The spec rules." (:168) and "4a. Check the store" (:221), at 426 lines.

## Check commands

- **Premise probes (all PASS at 51fd3b2b).**
  - Task 1: `grep -q 'pub(crate) fn load_paths' tomlctl/src/ui_capture.rs`.
  - Task 2: `grep -q 'pub(crate) const SCHEMA_VERSION: i64 = 4' ... && grep -q 'Reconcile => "reconcile"' ...`.
  - Task 3: the closure_down / closure_up greps.
  - Task 6: `grep -q '^version = "0.24.0"' tomlctl/Cargo.toml`.
  - Task 8: the out_of_lane and exempt greps.
  - Task 15: `` grep -q '^## 11. The `needs_human` report' .../drain.md ``.
- **Worker-run forward checks.** Each task's `Writes build: no` check (tasks 1-20, exactly as written in the worker view) was run on the present tree, and every one printed FAIL. That is the expected answer before the change, so each one discriminates. The narrowest conjuncts also fail now: `grep -q 'requests' lane.rs`, `grep -q 'granted' lane.rs`, `grep -q 'request' plan-store.md` and `grep -q 'generated:' tests/plan_emit.rs`.
  - Task 8's check (`grep -q 'granted' && grep -q 'requests'`) is weak: any comment holding both words passes it. Its nextest accept is what proves the behaviour.
- **Task 18 guard.** `bash scripts/verify-shared-blocks.sh` passes on the present tree. It is a guard and is expected to stay passing.
- **Not run.** The build-writing checks: the nextest filters for tasks 1-12, and `cargo test ... --lib -- cli::dispatch::tests` for tasks 13-19. Each compiles the crate, and all are gate-owned. F2's failure is inferred by reading finding_classes.rs, not observed. Running the dispatch subset after tasks 11, 12 and 14 land would settle it.

## Coverage

- **Tasks read:** 1-20, each through the worker view and `tasks show --select id,files`.
- **Files read or probed:** tomlctl/src/ui_capture.rs, lib.rs, plan/schema.rs, plan/records.rs, plan/target.rs, plan/store.rs, plan/mod.rs, plan/lane.rs, plan/emit.rs, plan/view.rs, plan/check.rs, plan/run_report.rs, io.rs, tasks/schema.rs, tasks/graph.rs, tasks/closure.rs, tasks/update.rs, cli/types/plan.rs, cli/types/mod.rs, plan/dispatch.rs, and cli/dispatch/tests/finding_classes.rs and skills.rs; tests it.rs, output_options.rs, plan_emit.rs and plan_check.rs; Cargo.toml and Cargo.lock; tomlctl/README.md; claude/commands/implement-v2.md; implement-v2-engine SKILL.md, drain.md, questions.md, run-milestone.md and apply-rows.md; flow-contract-task-store SKILL.md; plan-v2-engine write-and-prove.md; the tomlctl skill's plan*.md references; both plan-worker agents; scripts/shared-blocks.toml.
- **Not checked:**
  - Whether the workspace compiles once task 2's variant is added. Inferred from the exhaustive matches; no build was run.
  - The design documents beyond the sections the worker view quotes.
