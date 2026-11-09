Verdict: not as written. Tasks 2-6 can be carried out, but two points stop the milestone. Task 1 cannot compile without editing a file it is not allowed to touch, and no task removes the stale `plan handover add` from flow.md, so the command_lint gate stays red after milestone A. One task-7 check cannot pass on a valid markdown table, and task 2 names the wrong function.

# Cold read: milestone A (tasks 1-7)

## Unactionable

### U1. Task 1: the re-export in `cli/mod.rs` is not in the task's files

- **Plan text:** task 1 says "replace `PlanHandoverOp::Add` and `PlanHandoverAddArgs` with `Set` and `PlanHandoverSetArgs`". Its constraint says "Touch no file outside this task's files". The file list is schema.rs, records.rs, cli/types/plan.rs, dispatch.rs, approve.rs and view.rs.
- **Code:** `tomlctl/src/cli/mod.rs:37` re-exports `PlanHandoverAddArgs` by name in its `pub(crate) use types::{...}` list. `cli/types/mod.rs:29` is `pub(crate) use plan::*;`, so once the struct is removed that name no longer resolves.
- **Effect:** the crate will not compile, and the task's own accept check `cargo test --manifest-path tomlctl/Cargo.toml --lib plan::schema` cannot pass. The worker has two choices, and the plan settles neither: break the constraint, or keep a dead `PlanHandoverAddArgs` alias. A dead alias would also defeat the accept check `! grep -q 'PlanHandoverAddArgs' tomlctl/src/cli/types/plan.rs` if it lived in that file.
- **Fix:** add `tomlctl/src/cli/mod.rs` to task 1's files.

### U2. No task covers `references/flow.md`, which still names `tomlctl plan handover add`

- **Code:** `claude/skills/tomlctl/references/flow.md:65` reads `` | `tomlctl plan handover add` | yes — checks `plan.toml` | ``.
- **Gate:** `command_lint` (`tomlctl/src/cli/dispatch/tests/lint.rs:396`) parses every backticked `tomlctl ...` in skill bodies and one level of `references/*.md` with the real clap parser. It fails on an unknown subcommand.
- **Coverage:** no task in the flow lists `flow.md` (checked with `tomlctl tasks list --slug human-only-handovers --select id,milestone,files`).
- **Effect:** task 7's Outcome says "so the command and finding-class gates pass", but task 7 may not touch flow.md. After milestone A the `cli::dispatch::tests` gate fails, and the pre-commit hook runs that gate on every staged `tomlctl/src/**` or `claude/**/*.md` path.
- **Fix:** add `claude/skills/tomlctl/references/flow.md` to task 7 (change the row to `tomlctl plan handover set`).

### U3. Task 7's SKILL.md check cannot match a valid table row

- **Plan text:** "SKILL.md: the quick-reference row reads `handover {set|done}`". Its check is `grep -q 'handover {set|done}' claude/skills/tomlctl/SKILL.md`.
- **Code:** the row at `claude/skills/tomlctl/SKILL.md:61` is a markdown table cell, so its pipes are escaped: `handover {add\|done}`. An unescaped `|` would split the cell.
- **On the present tree:** `grep -q 'handover {add|done}'` returns no match, and `grep -qF 'handover {add\|done}'` returns a match. The check therefore fails on the correctly escaped edit `handover {set\|done}`. The worker must either break the table or fail the accept check.
- **Fix:** change the check to `grep -qF 'handover {set\|done}'`.

## False premises

### F1. Task 2: routing does not happen in `next_for`

- **Plan text:** task 2 says "route both `plan/handover-task-unknown` and `plan/handover-incomplete` to it in `next_for`".
- **Code:** `next_for` is a test helper inside `mod tests` at `tomlctl/src/plan/stage.rs:296`. Classes are routed in `fn remedy` (`stage.rs:115-140`, `"plan/handover-task-unknown" => HANDOVER_REPLACE` at :138).
- **Also missed:** `HANDOVER_REPLACE` appears in `PROVE_ORDER` (`stage.rs:41`), which the worker must update when the constant is replaced.
- **Severity:** a worker can recover by reading the file, so this does not block on its own.

## Premises confirmed

- schema.rs:
  - `SCHEMA_VERSION: i64 = 1` is at :27.
  - `Handover { id, command, source_task, done }` is at :299.
  - `to_toml` writes the `schema_version` read from disk (:379-395).
  - The `vocabulary!` macro is used for the other enums.
  - The required-key test (:1013) and the quoted-flag test (:1035) name handover keys.
  - A `Handover` literal in a unit test at :1203 also needs the new fields. It is in the same file, so it is in scope.
- records.rs:
  - `contract`/`upsert_contract` is at :254 and :268 and is a clean model for `handover_set`.
  - `handover_add` is at :334 and `handover_done` at :352.
- `dispatch.rs:16` routes `Add`.
- approve.rs:
  - `h.command` is read at :99.
  - `"kind": "handover"` is chained into `stops` at :95-103.
- view.rs: `handover.command` is read at :219 (markdown) and :425 (`undone_handovers`).
- check.rs:
  - `handover_finding(&Handover, &Store) -> Option<Finding>` is at :300 and is called with `filter_map` at :73.
  - `contract_findings` returns `Vec` at :269.
- serde_json has `preserve_order` (`tomlctl/Cargo.toml:28`), so "handovers directly after stops" in task 3 is meaningful.
- The design reference `design/handover.md#reasons` / `#shape` resolves to `docs/flows/human-only-handovers/design/handover.md` and holds the six reasons and the field table.
- Every test file named by tasks 4-6 exists. The handover sites match the Detail:
  - plan_records.rs:104, 176, 344, 357 and 401
  - capabilities.rs:155
  - output_options.rs:317
  - plan_check.rs:446 and 460
  - plan_stage.rs:17 and 699-723
  - plan_approve.rs:806 and 953
  - plan_view.rs:99-107 and 561
  - plan_lifecycle.rs:390, 574 and 716
- No other test file under `tomlctl/tests/` mentions handover.
- No test asserts a written plan.toml's `schema_version = 1`, so the version bump has no unlisted test fallout that I could find.

## Minor points that do not block

- **Task 3:** the markdown line template `<action>` does not say whether the action stays wrapped in `code()` as the command is today (view.rs:219). Task 6 follows task 3's code, so the worker's choice is safe.
- **Task 1:** it does not say whether an update row may set `action` to an empty string. `upsert_contract` refuses an empty `claim` through `non_empty`, so a worker would likely mirror that.

## Check commands (run on the present tree, all read-only)

Premises, expected to pass at base:

| Command | Result |
|---|---|
| `grep -q 'pub(crate) command: String' tomlctl/src/plan/schema.rs` | PASS |
| `grep -q 'fn upsert_contract' tomlctl/src/plan/records.rs` | PASS |
| `grep -q 'HANDOVER_REPLACE' tomlctl/src/plan/stage.rs` | PASS |
| `grep -q '"kind": "handover"' tomlctl/src/plan/approve.rs` | PASS |

Forward acceptance checks, all failing before the change as they should:

| Task | Command | Result |
|---|---|---|
| T1 | `grep -q 'SCHEMA_VERSION: i64 = 2' ...schema.rs` | FAIL |
| T1 | `grep -q 'done_when' ...schema.rs && ! grep -q 'pub(crate) command: String' ...` | FAIL |
| T1 | `grep -q 'PlanHandoverSetArgs' ...cli/types/plan.rs && ! grep -q 'PlanHandoverAddArgs' ...` | FAIL |
| T2 | `grep -q '"plan/handover-incomplete"' ...check.rs` | FAIL |
| T2 | `! grep -q 'plan handover add' ...stage.rs` | FAIL |
| T3 | `grep -q '"handovers"' ...approve.rs` | FAIL |
| T3 | `! grep -q '"kind": "handover"' ...approve.rs` | FAIL |
| T3 | `grep -q 'done_when' ...view.rs` | FAIL |
| T4 | `! grep -q '"--command"' tomlctl/tests/plan_records.rs` | FAIL |
| T5 | `grep -q 'plan/handover-incomplete' tomlctl/tests/plan_check.rs` | FAIL |
| T6 | `grep -q 'done_when' tomlctl/tests/plan_approve.rs` | FAIL |
| T7 | plan.md set/incomplete/no-add check | FAIL |
| T7 | `! grep -q 'Not checked against the rows' ...plan-store.md` | FAIL |
| T7 | `grep -q 'handover {set|done}' ...SKILL.md` | FAIL. It cannot discriminate: see U3. |
| C2 | contract check | FAIL (the write-and-prove.md part belongs to task 11, outside this milestone) |

The cargo checks (T1 `--lib plan::schema`, T2 `--lib plan::stage`, and the T4, T5 and T6 `cargo nextest ... binary(...)` filters) were not run, because they would take the shared build lock. cargo-nextest 0.9.136 is installed, so the `binary()` filters are available. U1 means T1's cargo check fails to compile as the plan is scoped.

## Coverage

- **Tasks:** I read the worker view and the `files` list for tasks 1-7, and the `files` list for every task in the flow (to look for flow.md and cli/mod.rs).
- **Sources read:**
  - schema.rs, records.rs, cli/types/plan.rs, cli/mod.rs, cli/types/mod.rs, dispatch.rs, approve.rs, view.rs, check.rs and stage.rs
  - grep of each test file named in tasks 4-6
  - SKILL.md, references/plan.md, plan-store.md and flow.md
  - cli/dispatch/tests/lint.rs
  - design/handover.md
- **Not read:** the other notes under notes/, per procedure.
- **Not checked:** whether the finding-class gate (`finding_classes.rs`) needs more than the plan.md table row.
