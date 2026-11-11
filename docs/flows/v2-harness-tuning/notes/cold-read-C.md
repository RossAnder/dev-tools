Verdict: Milestone C cannot be carried out exactly as written. Three points stop it, all of the same kind: a task has to edit a dispatch file its `files` leave out, which breaks its own "Touch no file outside this task's files" rule. There is also one minor false premise, and one accept check is already green on the present tree.

# Cold read: milestone C (tasks 18 to 25)

## Unactionable points

### U1. Task 19: the `reproduce-status` selector cannot reach `status` without editing `review/dispatch.rs`

Task 19's Action is "Add the `--flow` and `--milestone` selector to `review reproduce-status`". Its Constraints say "Touch no file outside this task's files". The files are `review/reproduce.rs`, `cli/types/review/verdicts.rs`, `review/schedule.rs` and the two test files.

The CLI args reach `reproduce::status` only through `tomlctl/src/review/dispatch.rs:35-40`. That call passes the fields by position: `reproduce::status(&target(&args.store)?, &args.ids, &args.expect, &args.integrity)`. `status` itself (`review/reproduce.rs:230`) takes `ids: &[String]`, and it refuses an empty list ("no finding ids given"). For `--flow`/`--milestone` to reach `status`, either that call site changes or a field of the args struct gets overloaded. `review/dispatch.rs` is not in the task's files, and the plan does not say which of the two it wants.

To settle it, add `tomlctl/src/review/dispatch.rs` to task 19's files.

### U2. Task 24: a new `AgentsOp` variant breaks the exhaustive match in `cli/dispatch.rs`

Task 24 says "`AgentsOp` is in `cli/types/cmd.rs`", which is true (`cmd.rs:400`). It lists `src/agents/dispatch.rs` as the dispatch file. But `AgentsOp` is matched in `tomlctl/src/cli/dispatch.rs:226-235`, which handles `Record` and `List`, has no wildcard arm, and calls `crate::agents::dispatch::dispatch_record` / `dispatch_list`. `src/agents/dispatch.rs` only holds the entry points, which take plain arguments ("the CLI layer only destructures and forwards", its module doc).

Adding `AgentsOp::Audit` leaves that match non-exhaustive, so the crate stops compiling until `cli/dispatch.rs` gains an arm. That file is outside the task's files and its no-other-files constraint.

To settle it, add `tomlctl/src/cli/dispatch.rs` to task 24's files.

### U3. Tasks 21, 22 and 23: who routes the `plan emit` subcommands is not settled

Task 21 says "register a `PlanOp::Emit` with a nested subcommand enum in `cli/types/plan.rs` and `plan/dispatch.rs`". Today `plan/dispatch.rs` matches every nested op itself (for example `PlanOp::Milestone { op } => match op { Add(..) => .., Update(..) => .. }`, lines 30-33). Tasks 22 (`gate-list`) and 23 (`reproduce-units`) each add a variant to that nested enum, but neither lists `tomlctl/src/plan/dispatch.rs` in its files.

They can only stay in lane if task 21 forwards the whole nested enum to a function in `emit.rs` (for example `PlanOp::Emit { op } => emit::dispatch(op)`). Task 21 does not say so, and if its worker follows the existing pattern in `plan/dispatch.rs`, tasks 22 and 23 cannot add their arms.

To settle it, task 21 should state that `plan/dispatch.rs` forwards the `Emit` enum whole to `emit.rs`. The alternative is to add `plan/dispatch.rs` to tasks 22 and 23.

## False premises

### F1. Task 21: "The HEAD read follows what `plan drift` does"

`plan drift` never reads the HEAD commit id. It shells `git log` over a `format!("{since}..HEAD")` range (`tomlctl/src/plan/drift.rs:106-108`) and checks the approved commit with `git cat-file -e` (`drift.rs:60-67`).

The pattern the task needs, comparing a probe's `at` against the current HEAD sha, is in `tomlctl/src/plan/approve.rs:397`: `rev-parse --verify HEAD^{commit}`. The premise script records `at` from `git rev-parse HEAD` (`plan/probes.rs:235`).

This is minor: a worker will find the right read. But the pointer sends them to a file that has none.

## Premises confirmed against the code

- Task 18:
  - `Accept` is at `tasks/schema.rs:271`. `writes_build` is a plain `bool`, and an absent key reads as `true` (`schema.rs:1126-1129`, `add.rs:599-600`), so "absent" and "true" behave the same, as `design/shapes.md#accept-owner` needs.
  - `accept_to_toml` already destructures the struct in full (`schema.rs:1135`).
  - Every named site exists: `ACCEPT_FIELDS`, `add.rs:99`; `accept_entry`, `add.rs:572`; `accept_json`, `show.rs:143`; `keep_probes`, `update.rs:526`, with the review-spec projection at `update.rs:1694`; `spec_block`, `view.rs:729`; `skip_reason`, `probes.rs:151`; the accept loop and `probe-absent`, `check.rs:486-510`; `entry_matches`, `store.rs:263`.
  - Every `Accept { .. }` literal in `src/` sits in the task's own files: `schema.rs`, `add.rs`, `update.rs` and `store.rs`. The other files that mention `writes_build` (`review/tier.rs`, `review/schedule.rs`, `review/add.rs`, `plan/target.rs`) mention it only as JSON or TOML text, so adding fields breaks no file outside the task.
  - `review/tier.rs:58` `PROBE_FIX_KEYS = ["check", "positive"]` confirms the allowlist claim.
  - All four test files exist.
- Task 19:
  - `Compose::accept` is at `schedule.rs:709` and carries `store_flag`, `flow` and `checkpoint` (`schedule.rs:437-445`).
  - The existing test asserts the per-finding string at `tests/review_schedule.rs:374`.
  - `FindingLink {ref, store, fixes, settles}` is at `tasks/schema.rs:242`.
- Task 20:
  - `MilestoneSpec {id, gates, elaboration, defer_reason}` is at `schema.rs:320`. Its literals are only in `plan/milestone.rs` and `schema.rs`, both in the task.
  - The `p` helper is at `probes.rs:70`, and `--gate` is in `cli/types/plan.rs:296,329`.
  - `tests/plan_milestone.rs` exists.
- Task 21:
  - Sections 3 (premises) and 4 ("Build the args", six rules) are in `run-milestone.md:77,136`.
  - findings.md has "Anchor premises on a started row" at line 33.
  - `Graph::closure_down` is at `tasks/graph.rs:266`.
- Task 23:
  - `Run` is at `plan/schema.rs:248` and `run_start` at `plan/records.rs:618`. `records::run_start` takes the whole args struct (`plan/dispatch.rs:20`), so `--tasks` needs no dispatch edit.
  - Findings carry an optional `route` (`review/store.rs:851`), and `review::store::load` is at `review/store.rs:1310`.
  - findings.md has "The units" at line 67.
- Task 25:
  - Every named file exists.
  - `plan_view_writes_build` is in `cli/types/mod.rs:182`, `tests/capabilities.rs:1830` and `README.md:380,569`.

## Check commands

All accept checks and premise checks were run from the repo root on the present tree (HEAD `c223ee76` plus the uncommitted flow-store edits).

Premises, all expected to hold, all `rc=0`:

- `grep -q 'writes_build is not false' tomlctl/src/plan/probes.rs`
- `grep -q 'ACCEPT_FIELDS' tomlctl/src/tasks/add.rs`
- `grep -q 'tomlctl review reproduce-status {} {} --expect gone' tomlctl/src/review/schedule.rs`
- `grep -q 'struct ReviewReproduceStatusArgs' tomlctl/src/cli/types/review/verdicts.rs`
- `grep -q 'gate' tomlctl/src/cli/types/plan.rs && grep -q 'struct MilestoneSpec' tomlctl/src/tasks/schema.rs`
- `grep -q 'struct Run' tomlctl/src/plan/schema.rs`
- `grep -q 'enum AgentsOp' tomlctl/src/cli/types/cmd.rs`

Accept checks, all forward, all expected to fail before the change:

- Task 18: all five `grep -q 'fn …'` checks return `rc=1`.
- Task 19: both return `rc=1`.
- Task 20: all three return `rc=1`.
- Tasks 21, 22 and 23 (`tests/plan_emit.rs`): every check returns `rc=2`, "No such file or directory". The file is new in task 21, so `rc=2` fails the check just as `rc=1` would.
- Task 23 (`run_start_stores_tasks` in `plan_records.rs`): `rc=1`.
- Task 24: all three return `rc=2`, because `tests/agents_audit.rs` is new.
- Task 25:
  - feature tokens: `rc=1`, and each of the five tokens counts 0 in `cli/types/mod.rs`
  - plan.md emit verbs: `rc=1`
  - tasks-store.md `run_by`/`covered_by`: `rc=1`
  - output-options: `rc=1`

Two checks are weak:

- **Task 25's output-options check cannot tell whether the `agents audit` case was added.** Its last clause, `grep -q '"audit"' tomlctl/tests/output_options.rs`, already matches the present tree (`grep -c` prints 1) because of the existing `&["backlog", "evidence", "audit"]` case at `tests/output_options.rs:741`. The whole check fails today only because of the `launch-args`, `gate-list` and `reproduce-units` clauses. Matching on `"agents", "audit"` would fix it.
- **Every accept check in this milestone only greps for a test name.** None proves the behaviour. The named narrow `cargo nextest` runs are the real proof, and they are left to the gate.

## Coverage

- **Tasks read:** 18 to 25, through their worker views and `tasks show --select id,files`.
- **Design read:** `docs/flows/v2-harness-tuning/design/shapes.md`, every section the tasks cite.
- **Files read:**
  - task 18's named sites in `tasks/schema.rs`, `tasks/add.rs`, `tasks/show.rs`, `tasks/update.rs`, `plan/view.rs`, `plan/probes.rs`, `plan/check.rs`, `plan/store.rs` and `review/tier.rs`
  - `review/reproduce.rs`, `review/schedule.rs`, `review/dispatch.rs` and `cli/types/review/verdicts.rs`
  - `plan/dispatch.rs`, `plan/drift.rs`, `plan/schema.rs`, `cli/types/plan.rs`, `cli/types/cmd.rs`, `cli/dispatch.rs`, `agents/dispatch.rs`, `agents/mod.rs` and `tasks/graph.rs`
  - `run-milestone.md` sections 3 and 4, and the findings.md headings
- **Not checked:**
  - Task 24's claim about the transcript JSON-lines shape (`message.content[].tool_use`, `input.command`). No Workflow transcript directory was in reach, and D13 cites one.
  - Whether the `implement-v2-milestone` Workflow accepts an extra `after` key in an args task. That falls to the D2 tasks outside this milestone.
  - No build or test was run; compile claims rest on reading the match sites.
