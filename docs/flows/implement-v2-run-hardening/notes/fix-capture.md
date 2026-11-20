Verdict: A visual fix task reaches its worker as a row with files, a copied look entry and design sections, but the worker view gives it no capture command, no `out` and no permission to capture (view.rs:819, plan-worker-deep.md:79). tomlctl already reads `.claude/ui-capture.toml` `paths` (ui_capture.rs:15, used at check.rs:894), so `plan/ui-accept-structural` (check.rs:866) only needs those globs passed in.

```toml sources
read_at = "1502e6b420be81caac7bc44ae0e369dc20e461c9"
dirty = []

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 147
claim = "Fix tasks: one per separable defect, reason prefix, files rule using the declaration's paths, one look entry in spec.accept, outcome, executor, design copied"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 33
claim = "The round directory is <out>/<id>-<r>/; the pass captures through the verification agent with --out and one --only per state and control"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 190
claim = "Every path under <out> is exempt from the clean-tree and out-of-lane checks and goes in no commit"

[[source]]
path = "claude/skills/implement-v2-engine/references/visual.md"
line = 28
claim = "The reason is counted from the [[changes]] array, so it lives in the change log, not on the row"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 231
claim = "New-task row shape: title, files, needs, reason and spec; it has no action or detail field"

[[source]]
path = "claude/skills/ui-capture/SKILL.md"
line = 28
claim = "--out creates the directory, --only is repeatable, an unknown --only state fails, exit 0 means every asked state was written; per-state png/txt and geometry.json"

[[source]]
path = "tomlctl/src/plan/view.rs"
line = 489
claim = "The worker view prints Intent, Non-goals, bound decisions, contracts, the task block, findings, cited design sections and handovers; it prints no change log and nothing from ui-capture.toml"

[[source]]
path = "tomlctl/src/plan/view.rs"
line = 819
claim = "A look entry prints only 'Run by: look' and 'States: ...'"

[[source]]
path = "claude/agents/plan-worker-deep.md"
line = 79
claim = "Shared plan-worker-procedure Checks: an entry marked Run by: look is never the worker's to run"

[[source]]
path = "claude/agents/plan-worker-deep.md"
line = 162
claim = "Forbidden ops: no edits outside the task's files besides the note and granted paths"

[[source]]
path = "tomlctl/src/ui_capture.rs"
line = 15
claim = "load_paths reads .claude/ui-capture.toml paths into a GlobSet; empty when the file or key is absent, Err when malformed"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 190
claim = "ui_findings(&tasks) gets no root; ui_scope_findings(&tasks, &root) does"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 866
claim = "ui-accept-structural fires on any pending row of a non-deferred ui_states milestone with no look entry and all accept entries writes_build = false; it never looks at the row's files"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 893
claim = "ui_scope_findings already matches row.files (normal_path) against the paths globs and turns a malformed config into one warning"

[[source]]
path = "tomlctl/src/review/survey.rs"
line = 586
claim = "The review survey is the second reader of load_paths"

[[source]]
path = "tomlctl/tests/plan_check.rs"
line = 1374
claim = "The existing ui-accept-structural test, whose fixture writes no capture config"

[[source]]
path = "claude/skills/plan-v2-engine/references/ui-layout.md"
line = 98
claim = "Original UI rows carry look entries too, so a look entry alone does not mark a task as a visual fix"
```

**How the fix task is minted.** The orchestrator stages a `tasks add-many` row per separable defect (visual.md:147-166): `files` from the look entry's task or from the milestone rows whose files match the declaration's `paths`, a `reason` opening `visual <id> round <r>: ` with the state, class, location and inspector note path, `spec.accept` holding one `run_by = "look"` entry (copied, or new and naming the state), `spec.outcome`, `spec.executor`, and `spec.design` copied. A capture-failure task instead carries a gate-owned `check` (visual.md:168-176).

**What the worker receives.** `plan view --for worker` (view.rs:489-547) prints the title, files, outcome, constraints, and for the look entry only `Run by: look` and `States: ...` (view.rs:819-822), plus the inlined design sections. It does not print the `reason`, which lives in `[[changes]]` (visual.md:28), so the state's defect class, location and inspector note path never reach the worker unless the orchestrator also writes them into the title, outcome or a constraint. Nothing in the view names `.claude/ui-capture.toml`, its `command` or `out`. The shared worker procedure says a `Run by: look` entry "is never yours to run" (plan-worker-deep.md:79), and edits outside the task's files are forbidden (line 162).

**What a self-capture would need.**
1. Permission and steps in the shared `plan-worker-procedure` block (lite and deep, byte-identical): for a look entry on a visual fix task, Read `.claude/ui-capture.toml`, run `<command> --out <out>/<id>-<r>-task-<n>/ --only <state>...` for the look entry's `States`, then read each `<state>.png`/`.txt` and `geometry.json` against `ui-capture/references/defects.md`. Controls are not needed: the worker is not the judge of record. The ui-capture contract covers the call already: `--out` creates the directory, `--only` repeats, and exit 0 means every asked state was written (SKILL.md:28-36).
2. A per-task directory that cannot collide with the pass's `<out>/<id>-<r>/` or the capture check's `<out>/<id>-capture-check/`. Every path under `<out>` is already exempt from the out-of-lane and clean-tree checks (visual.md:190-192), but the worker's own rule at line 162 still needs a matching carve-out.
3. A signal that the task is a visual fix. A look entry is not enough, because original UI rows carry look entries too (ui-layout.md:98), and the brief limits capture to fix tasks. Three candidates: the view prints the `visual <id> round <r>:` change reason; a spec constraint the orchestrator writes at mint time; or a printed capture line driven by that reason.
4. The defect itself, meaning the state, the class, the location and the inspector note path, printed where the worker reads it.
5. The brief's line 37 rule, worded for workers: a capture exit that names none of the worker's own files is noted and returned, not repaired.

**ui-accept-structural.** `ui_capture::load_paths` already reads and validates `paths`, and `ui_scope_findings` already matches `row.files` against those globs through `normal_path` (check.rs:893-930). `ui_findings` is called without `root` (check.rs:190), so the change is local: load the globs once, pass them to both functions, and add a files-match condition at check.rs:868. The planner has to decide what happens when there is no config or no `paths`. If an empty GlobSet means "match none", the warning goes silent everywhere. If it means "no filter", today's behaviour stays. It also has to decide whether a malformed config should still produce the one `ui-milestone-no-states` warning. A visual look-defect fix task never draws this warning, since it holds a look entry. The rows that do draw it are gate fix tasks and capture-failure fix tasks, which matches B-d09117ec.

- **Library/API or file**: tomlctl/src/plan/view.rs:819 `spec_block`; claude/agents/plan-worker-deep.md:79 (shared block `plan-worker-procedure`)
- **Source**: file:line reads at HEAD 1502e6b4
- **Evidence-grade**: high
- **Finding**: A visual fix worker gets no capture command, no output directory, no defect detail and no licence to capture, so self-capture needs view and procedure changes together. needs-plan
- **Details**: The look entry prints only `Run by: look` and `States:`. The `reason` carrying the state, class and note path is a `[[changes]]` row that the view never prints. The procedure forbids running look entries and editing outside the task's files.
- **Counter**: If the worker view printed `[[changes]]` reasons, or `spec_block` printed the capture declaration, the worker would already have the inputs. A grep of view.rs for `capture` and `changes` in the worker path found neither.
- **Impact on plan / report**: Touches view.rs (or a mint-time constraint in visual.md), the `plan-worker-procedure` block in plan-worker-lite.md and plan-worker-deep.md, visual.md Fix tasks, and the plan.md worker-view description.

- **Library/API or file**: tomlctl/src/plan/check.rs:866 `ui_findings`; tomlctl/src/ui_capture.rs:15 `load_paths`
- **Source**: check.rs:190-191 call sites; check.rs:893-930 `ui_scope_findings`
- **Evidence-grade**: high
- **Finding**: Silence `plan/ui-accept-structural` for rows whose files match no `paths` glob by passing the loaded GlobSet into `ui_findings`.
- **Details**: `load_paths` is the shared reader, also used by review/survey.rs:586. Add `row.files.iter().any(|f| globs.is_match(normal_path(f)))` to the condition at check.rs:868. Tests go in tomlctl/tests/plan_check.rs beside line 1374 with `write_capture_config`. The ui-accept-structural row in claude/skills/tomlctl/references/plan.md:574 changes wording.
- **Counter**: If an empty or absent `paths` is meant to keep the warning, the condition must skip filtering when the GlobSet is empty. Otherwise every project without `paths` loses the warning.
- **Impact on plan / report**: One task, three files: check.rs, plan_check.rs and references/plan.md.

Open risk: the contract says nothing about running the capture command concurrently, for example a fixed dev-server port. Two file-disjoint fix workers in the rolling pool could capture at the same time. This is project-dependent and no code settles it. A note on the capture command in the ui-capture skill would.

Coverage: read in full visual.md, ui-capture/SKILL.md, ui_capture.rs and plan-worker-deep.md. Read check.rs:770-947, view.rs:489-547, 720-743 and 795-842, drain.md:231-275, plan.md:367-391, plan_check.rs:1285-1384, backlog B-d09117ec and .claude/ui-capture.toml. Grep checks: `ui-capture` across tomlctl (readers: check.rs and survey.rs only), and `capture` in view.rs (none). Ruled out: emit.rs as a worker-facing source, since gate-list's `visual` object goes to the orchestrator only.
