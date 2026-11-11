Verdict: Not as written. Tasks 26, 31 and 32 can be carried out, but 27, 28, 29 and 30 stop on 2 unactionable points and 4 false premises: the scripts cannot see `same_as`, `review reproduce` has no `--by` flag, a worker's own premise emit is empty once its row is `in-progress`, and the checker's reproduce return and payload location are never settled.

# Cold read: milestone D (tasks 26-32)

## Unactionable

### U1. Task 28 does not say what the checker returns in reproduce mode
- Task 28, Detail: "dispatch `review-checker` with a prompt that names mode reproduce, the store, flow, milestone, round and the finding ids, and run `runUnit` for each unit holding a finding the checker returned `unsettled` or did not return".
- Missing: the `schema` for that `agent()` call and the shape of the return. The wave-mode checker returns `batchSchema` `{verdicts: [verdict_return]}` (`claude/workflows/review-v2.js:175-179`). The reproduce verifier returns `unitSchema` `{task, reproductions: [{finding, verdict}]}` (`review-v2.js:81-96`). Task 28 picks neither.
- Task 30 (Detail: "return the finding as `unsettled`") does not name the shape either. Tasks 28 and 30 are file-disjoint with no `needs` edge, so they can run in parallel, and each worker would have to guess the contract the other one writes.
- Task 32's reproduce-mode case needs a canned checker return in that same unknown shape.
- `design/shapes.md#reproduce-checker` does not settle it.

### U2. Task 30 does not say where the checker stages its payload
- Task 30, Detail: the checker records `gone` "by `tomlctl review reproduce` ... citing the grep".
- `review reproduce` takes its rows only through `--ndjson <SRC>` (see F3). The `review-v2-store-writer` block that the checker carries says "Write each payload with the Write tool to `payloads/<cell or batch id>.ndjson` in the run directory ... never pipe, echo or heredoc a payload" (`claude/agents/review-checker.md:55`).
- The checker prompt that Task 28 specifies names no run directory, so the checker has nowhere to write its payload. The verifier's reproduce prompt does carry `run directory:` (`review-v2.js:110`).
- The checker's prompt also names no per-finding `task` ref. That key is optional, but the verifier always writes it.
- Neither task says whether the checker prompt carries a run directory, or how the NDJSON file is named.

## False premises

### F1. Task 28 assumes `routeCell` can see `same_as`
- Claim (Task 28): "when `args.batch_defects` is true and a cell has two or more such singles without `same_as` ... keep a row with `same_as` on `verifyOne`".
- What the code shows: `routeCell` sees only the cell-return findings (`review-v2.js:277-301`). That schema has `additionalProperties: false` and only `id`, `kind`, `severity` and `route` (`tomlctl/src/review/schema_out.rs:250-276`). `review add` builds the same four keys (`tomlctl/src/review/add.rs:240-245` and `cell_return_entry` at `:871`).
- No task in the plan adds `same_as` to the cell return (searched `plan.toml`, `tasks.toml` and `design/shapes.md`). The script cannot tell which rows carry `same_as`, and it has no shell to read the store.

### F2. Task 29 assumes the plan wrapper knows which reverify ids carry `same_as`
- Claim (Task 29): "keep an id that carries `same_as` in a dispatch of its own".
- What the code shows: the reverify ids reach `plan-v2-review.js` only as bare strings from the lead's settle return (`claude/workflows/plan-v2-review.js:118`, `reverify: IDS`; consumed at `:372-378`). Nothing in the script's inputs says which id has `same_as`.
- To carry this out, the settle schema or the lead's return would have to change. Task 29 forbids that ("The compose, settle and audit phases are unchanged").

### F3. Task 30 assumes `review reproduce` has a `--by` flag
- Claim (Task 30): "by `tomlctl review reproduce` with `--by checker`". The accept check is `grep -q -- '--by checker' claude/agents/review-checker.md`.
- What the code shows: `tomlctl review reproduce --help` lists only `--slug`/`--scope`, `--ndjson <SRC>`, the integrity flags and the output flags. `by` is an NDJSON key (`claude/skills/tomlctl/references/review-apply.md:124`).
- Task 14 (milestone B) adds `checker` as a `ReproduceBy` value, not a flag. Its Outcome line repeats the same `--by checker` wording.
- A fenced `tomlctl review reproduce --by checker` in an agent file would also be checked by the dispatch command lint, which scans `claude/agents` (`tomlctl/src/cli/dispatch/tests/lint.rs:46`). That lint is the check command Task 30 itself names.
- The accept grep can pass without a reproduce procedure, for example on a `review verdict --by checker` line.

### F4. Task 27's premise command prints nothing once the worker's row is `in-progress`
- Claim (Task 27, step 1): when told to, the worker runs `tomlctl plan probes --slug <slug> --emit --premises --task <id>`, runs the script, and fails "when a premise line exits non-zero".
- What the code shows: `--emit` covers only pending rows. `task_skip_reason` returns "status is not pending" for any other status (`tomlctl/src/plan/probes.rs:187-189`) and writes only a `# skipped task N` comment (`:243-245`).
- The shared procedure marks the row `in-progress` at step 5, before the work (`claude/agents/plan-worker-lite.md:28`). A restarted worker (step 4) also finds its row already `in-progress`.
- So the emitted script has no `p` line for the task, and every after-premise passes without being checked. The task never says the check must run before step 5, and nothing covers the restart case.
- Smaller point: the procedure gives the worker no scratch directory. The worker prompt (`implement-v2-milestone.js:130-140`) names none, and `plan-worker-lite.md` has no "scratch".

## Minor points (not counted)
- Task 28: "The mode's index ... counts every agent launched". The reproduce index has no `agents` key today (`review-v2.js:50`), and the Constraint says route-less args "behave exactly as today". It is not stated whether to add the key.
- Task 32: the pre-commit regex is `claude/workflows/.*\.js` (`.githooks/pre-commit:58`), which does not match `harness.test.mjs`. A commit that touches only the harness does not trigger the dispatch subset; the full suite still runs it.
- Task 32 premise "the Workflow-script lint reads only the top level": the check `grep -q 'fn command_lint_workflow_scripts'` cannot tell whether that is true. The claim itself is true: `read_dir` is non-recursive and keeps only the `.js` extension (`lint.rs:151-160`).
- Task 26 can be carried out as written. The checker-pool precedent for `agent()` outside `parallel()` is at `review-v2.js:240-259`. `nextWave`, `parallel`, `settleBlocked`, `toEntry`, `RETURN_SCHEMA` and `SCRIPT_VERSION` are all where the Detail says (`implement-v2-milestone.js:18-250`).
- Task 31 can be carried out as written. The envelope row "one verifier per critical or major defect" is at `claude/skills/review-v2-engine/SKILL.md:219`.

## Check commands (run on the present tree, read-only)
| Task | Command | Result |
|---|---|---|
| 26 premise | `grep -q 'nextWave' ... && grep -q 'await parallel' ...implement-v2-milestone.js` | rc=0 (holds) |
| 26 | `grep -q 'Promise.race' claude/workflows/implement-v2-milestone.js` | rc=1 (discriminates) |
| 26 | `grep -q 'needs_files' claude/workflows/implement-v2-milestone.js` | rc=1 |
| 26 | `! grep -q 'nextWave' claude/workflows/implement-v2-milestone.js` | rc=1 |
| 27 | `grep -q 'needs_files'` on both worker files | rc=1 |
| 27 | `grep -q 'after-premises'` on both worker files | rc=1 |
| 27 | `grep -q 'Run by: worker'` on both worker files | rc=1 |
| 28 premise | `grep -q 'runUnit' ... && grep -q 'CHECKER_BATCH' claude/workflows/review-v2.js` | rc=0 |
| 28 | `grep -q 'batch_defects' claude/workflows/review-v2.js` | rc=1 |
| 29 premise | `grep -q 'workflow(' ... && grep -q 'reverify' claude/workflows/plan-v2-review.js` | rc=0 |
| 29 | `grep -q 'batch_defects: true' claude/workflows/plan-v2-review.js` | rc=1 |
| 30 | `grep -q -- '--by checker' claude/agents/review-checker.md` | rc=1; the string it greps for names a flag that does not exist (F3) |
| 31 | `grep -q 'batch_defects'` on SKILL.md and agent-waves.md | rc=1 |
| 32 | `grep -q` of the three script names in `claude/workflows/tests/harness.test.mjs` | rc=2 (file absent, as expected before the task) |
| 32 | `grep -q 'fn workflow_scripts_logic_passes_under_node' .../lint.rs` | rc=1 |
| 32 premise | `node --version` | `v26.13.0` |
| 32 premise | `grep -q 'fn command_lint_workflow_scripts' .../lint.rs` | rc=0 (does not test the claim) |

Not run: `cargo test ... cli::dispatch::tests`, `bash scripts/verify-shared-blocks.sh` and `node --test` (crate-wide, or they need the change); left to the gate.

## Coverage
- Tasks: worker views and files for 26-32.
- Read: `claude/workflows/implement-v2-milestone.js` (whole file); `claude/workflows/review-v2.js` (reproduce block, verify and route); `claude/workflows/plan-v2-review.js` (reverify, pipeline call, baselines); `claude/agents/plan-worker-lite.md` (procedure); `claude/agents/review-checker.md` and `review-verifier.md` (store-writer and reproduce); `scripts/shared-blocks.toml`; `tomlctl/src/plan/probes.rs`; `tomlctl/src/review/schema_out.rs` and `add.rs`; `tomlctl/src/review/fields.rs` (checker sees `old`); `lint.rs`; `.githooks/pre-commit`; `design/shapes.md`; `SKILL.md:210-220`.
- To check premises: the spec of task 14 and the task titles and `needs` of tasks 15, 16, 21 and 33.
- Not checked: Windows node availability, which D3 records as unobserved.
