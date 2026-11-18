Verdict: Make `.claude/generated.toml` an array of `[[generators]]` tables (`id`, `inputs` as globs, `outputs` as literal paths, `regenerate`, `check`), with a loader in a new `tomlctl/src/generated.rs` built like `ui_capture::load_paths` (tomlctl/src/ui_capture.rs:15). Three places read it: a new check class registered beside `ui_scope_findings` (tomlctl/src/plan/check.rs:187), a `generated` source appended after the accept checks in `gate_list` (tomlctl/src/plan/emit.rs:232), and the scope-request verb that sibling lenses own. That verb calls the same loader, and nothing in the tree reads the file yet.

## Answer

**Shape.** Two configs already exist to copy from. `.claude/ui-capture.toml` is a flat table with a `paths` glob array. The project layer's `[[checks]]` is an array of tables carrying `id`, `paths` and `command` (claude/skills/review-v2-lenses/SKILL.md:191). A project can have several generators, so the right model is the `[[checks]]` row:

```toml
[[generators]]
id = "canvas-css"                         # unique; names the gate-list source and findings
inputs = ["packages/canvas/src/tokens.css", "packages/canvas/src/tokens/**"]   # globs, `*` stops at `/`
outputs = ["packages/canvas/src/canvas.generated.css"]                         # literal repo-relative paths
regenerate = "bun run gen:canvas"         # run from repo root by the worker that edits an input
check = "bun run gen:canvas --check"      # exits non-zero when outputs are stale; enters the gate list
```

- `inputs` take globs, compiled the way ui_capture.rs compiles them: `./` trimmed, `literal_separator(true)`. A row's `files` are concrete paths, so the test is `globset.is_match(normal_path(file))`, as in `ui_scope_findings`.
- `outputs` must be literal paths, never globs. The grant rule and the plan check have to name the exact path to add to a row's `files`, and a row cannot hold a glob.
- `check` should be required. Without it the gate cannot notice a stale output. `regenerate` should also be required, because the worker that holds the output needs to know how to rebuild it. The loader should refuse duplicate `id`s and unknown keys, as the catalogue parser does (tomlctl/src/review/catalogue.rs:2187-2200, `"is not a config key"`).
- A missing file means "no generators" (`NotFound` returns empty, ui_capture.rs:18-19). A malformed file should give one warning finding that names no row, following ui-capture's practice at check.rs:888-897. It should not refuse.

**Readers.**
1. **`plan check`.** Add a new warning class, for example `plan/generated-output-unheld`, built and registered beside `ui_scope_findings` in `findings()`. `root` is already resolved at check.rs:172. It reports a pending row in a non-deferred milestone whose `files` match a generator's `inputs` while missing one of its `outputs`, and `ids` names that row. The test is per row, not per milestone: the worker runs `regenerate` inside its own lane, so an output held by a sibling row is still an out-of-lane write and a concurrency hazard. Warning rather than error follows the precedent that the config belongs to the project, not the plan (check.rs:884-886). The run-time grant also backs it up. The class needs a row in claude/skills/tomlctl/references/plan.md's class table (beside :573) and tests in tomlctl/tests/plan_check.rs. Its config fixture can copy the ui-capture fixture writer there (:1293).
2. **`plan emit gate-list`.** `gate_list` (emit.rs:172) loads only the task store and never resolves a root. It needs `repo_or_cwd_root()` plus the loader. After the accept-check loop it should add one `{"command": check, "source": "generated:<id>"}` for each generator with an input matching any `files` entry of any member row. These go through the existing `listed` set (emit.rs:182, :191), so a gate or accept check that already runs the same command is not run twice. Add tests to tomlctl/tests/plan_emit.rs beside `gate_list_orders_gates_then_gate_owned_checks` (:671), and document the new `source` value in plan.md. "Touched" here means a member row holds a matching input, which is what the store records. Reading the run's actual diff would need git, and no emit leaf does that today.
3. **The run-time scope-grant rule.** The tree has nothing to attach this to. Scope requests are decided today in prose at the drain ("Widening a row from `needs_files`", claude/skills/implement-v2-engine/references/drain.md:197). The request record and its verb belong to the `request-record` and `drain-integration` lenses. The generated-map part of the rule should be one shared function in `generated.rs`, for example `outputs_of(held: &[String]) -> BTreeSet<String>`. Whichever verb answers a scope request calls it, so the orchestrator never matches globs in prose. `plan check` and `gate-list` use the same function, which keeps all three readers in agreement, just as `ui_capture` is shared by `plan check` and the review survey (ui_capture.rs:1-2; survey.rs:586). One option sits outside this lens: `out_of_lane` (tomlctl/src/plan/lane.rs:423) could treat the outputs of a dispatched row's inputs as in-lane, using the same function. The drain-integration lens owns that decision.

## Findings

- **Library/API or file**: tomlctl/src/ui_capture.rs:10 `CONFIG`, :15 `load_paths`
- **Source**: file read in full (41 lines); consumers at tomlctl/src/plan/check.rs:36 and :888, tomlctl/src/review/survey.rs:586
- **Evidence-grade**: high
- **Finding**: The pattern for a per-project config shared by `plan check` and another reader is a small top-level module with a `CONFIG` const and a loader that returns empty on NotFound and `Err(String)` otherwise. `generated.rs` should be a sibling built the same way, with its `mod` line beside lib.rs:42.
- **Details**: `load_paths` trims `./`, builds each glob with `GlobBuilder::literal_separator(true)` and maps parse errors to a single line. Commit 5129de82 ("share the ui-capture config loader") shows this module was extracted so the two readers would agree, which is the same reason to share here.
- **Counter**: The planner would want a different home if the generator map should merge with `.claude/review-lenses/config.toml`'s `[[checks]]`. The brief fixes the file name as `.claude/generated.toml`, though, and that layer's parser refuses unknown keys (catalogue.rs:2200).
- **Impact on plan / report**: New file `tomlctl/src/generated.rs` plus a `mod` line in `tomlctl/src/lib.rs`, and a skill or reference section documenting the file (no owner exists yet; the ui-capture skill is the template).

- **Library/API or file**: tomlctl/src/plan/check.rs:148 `findings`, :187 `ui_scope_findings` call, :887 `ui_scope_findings`
- **Source**: file read at :140-200 and :860-945; claude/skills/tomlctl/references/plan.md:573
- **Evidence-grade**: high
- **Finding**: Register the new class in `findings()` beside `ui_scope_findings`, which already has `root`. Make it a warning, with a malformed config reported as one finding naming no row.
- **Details**: Approval blocks only on `ERROR` (tomlctl/src/plan/approve.rs:347), so a warning leaves the planner to judge. Matching should use `normal_path(file)` against the input globs, as at check.rs:916-918. Scope it to pending rows outside deferred milestones, as `ui_findings` does with `row.status == Status::Pending` and `is_deferred`.
- **Counter**: Make it an error instead if the planner wants approval blocked. That is a policy choice the code does not decide, and the brief only says "flags".
- **Impact on plan / report**: One task covering check.rs, plan.md's class table and tests/plan_check.rs.

- **Library/API or file**: tomlctl/src/plan/emit.rs:172 `gate_list`
- **Source**: file read at :160-240; tomlctl/tests/plan_emit.rs:659-727; tomlctl/src/cli/types/plan.rs:714 `PlanEmitGateListArgs`
- **Evidence-grade**: high
- **Finding**: `gate_list` builds `commands` from milestone gates, then from gate-owned accept checks, deduplicated through `listed`. Generator checks go after these with `source = "generated:<id>"`. This needs a repo root, which the function does not resolve today.
- **Details**: No new CLI flag is needed (`PlanEmitGateListArgs` stays as it is), so tests/output_options.rs needs no new case. The JSON `commands[].source` vocabulary gains a value, and the engine's gate step and plan.md (:534 onward) should say so.
- **Counter**: The design changes if "touched" has to mean "edited in this run" rather than "held by a member row". The diff would then come from `uncommitted_files` (lane.rs imports it from drift.rs), which no emit leaf reads today.
- **Impact on plan / report**: One task covering emit.rs, plan.md and tests/plan_emit.rs.

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:197; tomlctl/src/plan/lane.rs:423 `out_of_lane`
- **Source**: drain.md read at the grep hits (:27, :32, :71, :151-156, :197-216); lane.rs read at :1-60 and :340-470
- **Evidence-grade**: medium (the verb the rule will live in does not exist yet)
- **Finding**: A grant today is a prose rule applied at the drain. The generated-map clause ("a declared output of a held input") should be a tomlctl function the future request verb calls, not prose.
- **Details**: `out_of_lane` builds the lane from dispatched or started-uncommitted rows' `files` plus a fixed exempt list (lane.rs:435). A granted output not yet written into `files` would show up as out of lane unless the drain writes the grant into the row first, or the lane learns about grants.
- **Counter**: If the request-record lens settles that grants are applied only by the drain's audited `tasks update` before lane-check, then lane.rs needs no change and the function is called only by the request verb.
- **Impact on plan / report**: Grant-verb placement is shared with the request-record and drain-integration lenses. This lens contributes only the shared `outputs_of` function.

Out of lens, one line: the worker needs each held generator's `regenerate` command. That could reach it through the worker view or a spec `means`, which the worker-protocol lens owns.

Coverage: read in full tomlctl/src/ui_capture.rs, .claude/ui-capture.toml, .claude/review-lenses/config.toml and docs/flows/mid-run-requests/brief.md. Read in part plan/check.rs (:140-200, :860-945), plan/emit.rs (:160-240), plan/lane.rs (:1-60, :340-470), review/catalogue.rs (:2170-2215), cli/types/plan.rs (:705-735), the ui-capture and review-v2-lenses skills' config sections, and drain.md's widening and lane sections. Checks: grep for `generated.toml` in tomlctl/src (0 hits), consumers of `ui_capture::`, and gate-list tests. Ruled out: glob `outputs`, because a grant needs a concrete path a row can hold, and folding the map into review-lenses `[[checks]]`, because the brief names the file and that parser refuses unknown keys.

```toml sources
read_at = "51fd3b2b88fa092fea7bfc510421e2a6c78f72b7"
dirty = ["docs/flows/mid-run-requests/brief.md"]

[[source]]
path = "tomlctl/src/ui_capture.rs"
line = 15
claim = "Shared per-project config loader pattern: NotFound is empty, globs with literal_separator"

[[source]]
path = ".claude/ui-capture.toml"
claim = "Flat per-project config with a paths glob array"

[[source]]
path = ".claude/review-lenses/config.toml"
claim = "Array-of-tables project config with id, paths, command"

[[source]]
path = "claude/skills/review-v2-lenses/SKILL.md"
line = 191
claim = "Project [[checks]] row fields id, families, paths, command"

[[source]]
path = "tomlctl/src/review/catalogue.rs"
line = 2187
claim = "Project config parser parses [[checks]] and refuses unknown keys"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 187
claim = "findings() registers ui_scope_findings with the repo root; the new class sits beside it"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 887
claim = "ui_scope_findings matches row files against project globs; a malformed config is one warning naming no row"

[[source]]
path = "tomlctl/src/plan/approve.rs"
line = 347
claim = "Approval blocks only on error-severity findings"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 172
claim = "gate_list builds commands from gates then gate-owned accepts, deduplicated through listed, with no repo root"

[[source]]
path = "tomlctl/tests/plan_emit.rs"
line = 671
claim = "Existing gate-list tests the new source extends"

[[source]]
path = "tomlctl/tests/plan_check.rs"
line = 1293
claim = "ui-capture config fixture writer to copy for generated.toml tests"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 423
claim = "out_of_lane lane is row files plus a fixed exempt list; grants must reach files or the lane"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 197
claim = "Scope widening is today a prose drain rule"

[[source]]
path = "claude/skills/tomlctl/references/plan.md"
line = 573
claim = "plan check class table where the new class is documented"

[[source]]
path = "docs/flows/mid-run-requests/brief.md"
claim = "Success criterion naming generated.toml and its three readers"
```
