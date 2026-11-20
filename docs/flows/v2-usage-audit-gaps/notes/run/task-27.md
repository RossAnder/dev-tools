Status: done — elaboration now reconciles hand commits against decisions, sources outside facts, records the UI call and cites the no-writes rule

```toml run-note
files_touched = ["claude/skills/plan-v2-engine/references/elaborate.md"]

[[deviations]]
what = "Stale decisions found by plan drift are asked in section 2 outside its four-question budget, and the UI call is recorded as a decision of class look"
why = "The detail sends every stale decision to the Surface round but the budget would silently drop some past four; class look matches ui-layout's own pick decision. Alternatives: count them inside the budget (risks an unasked stale decision) or class ux (D8's own class, but that names the plan-level capture question, not a milestone's call)"
means = "detail"

[[deviations]]
what = "plan/brief-changed raised by the elaboration's own brief correction is exempted in 4a, section 5's re-check, the section 1 precondition on a resume and section 7, and settled by plan approve --amend after section 6's commit, then a second commit by approve.md section 7"
why = "approve.md requires brief.md committed before the stamp and plan check treats plan/brief-changed as an error, so the amend cannot run before the elaboration commit; without the exemptions the procedure would stop on its own edit"
means = "detail"

[[checks]]
command = "grep -q 'affected_decisions' claude/skills/plan-v2-engine/references/elaborate.md"
result = "pass"
```

## What changed

`claude/skills/plan-v2-engine/references/elaborate.md`:

- Section 1 gains "Read what was changed by hand": run `tomlctl plan drift --slug <slug>` (linked to `tomlctl/references/plan-engine.md#plan-drift`), read each `affected_decisions` entry's decision and its `commits` with `git show`, against `binds_hit`, and carry each decision whose text no longer holds, with the brief lines naming the same choice, into section 2.
- Section 2 gains "Stale decisions" (asked with the commits as evidence; on a yes, `plan decide` re-issues the decision and the brief is corrected inside this elaboration, the yes standing as approve.md's Amend agreement), "The UI call" (ui-layout.md is opened for every elaboration, the call recorded as a decision and with `plan milestone update --ui yes|no --reason`), and "Outside facts" (cites write-and-prove.md section 1's source rule, with pins probed by `--gate-probe` or a local premise before the flip).
- Section 4c cites open-and-challenge.md section 3's "No repo writes while a launch is open" rule between snapshot and check.
- Sections 4a and 5 exempt `plan/brief-changed` from the elaboration's own brief edit; section 6 stages `brief.md`, then runs `plan approve --slug <slug> --amend --review <outcome>` after the commit and commits the stamp's writes by approve.md section 7; section 7 treats the resume case.

Every verb and flag named was checked against `claude/skills/tomlctl/references/plan.md` (`plan milestone update --ui`, `plan approve --amend --review`) and `plan-engine.md` (`plan drift`, `affected_decisions`). The CLI dispatch test is gate-owned and was not run. The row was marked in-progress after the edits rather than before them, an ordering slip with no effect on the result.
