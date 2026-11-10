Status: done — the nine plan-family catalogue entries now direct finders to `blocked_on` instead of `fix_shape`

## What changed

One sentence per file, frontmatter and all other sentences untouched:

- `claude/skills/review-v2-lenses/lenses/plan-executor.md`: an unmade decision needing the author is a defect with `blocked_on = "decision"`, `blocked_missing` and at least two `blocked_options`, each naming its consequence.
- `claude/skills/review-v2-lenses/lenses/feasibility.md`: an unsettled contradiction or infeasible design is a defect with `blocked_on = "decision"`, with the same detail.
- `claude/skills/review-v2-lenses/lenses/plan-structure.md`: restructuring a prose plan is a defect with `blocked_on = "decision"` (it turns on task boundaries); it names the open boundary in `blocked_missing` and at least two candidate splits in `blocked_options`.
- `claude/skills/review-v2-lenses/lenses/plan-grounding.md`: missing work needing its own task gets `blocked_on = "design"` with `blocked_missing` and `blocked_contract` (the task the plan must gain).
- `claude/skills/review-v2-lenses/lenses/plan-premortem.md`: a prevention needing a new task or design change gets `blocked_on = "design"` with `blocked_missing` and `blocked_contract`.
- `claude/skills/review-v2-lenses/lenses/plan-risk.md`: a compatibility layer or scope gap needing its own task gets `blocked_on = "design"` with `blocked_missing` and `blocked_contract`.
- `claude/skills/review-v2-lenses/checks/acceptance-contradiction.md`, `checks/falsifiability.md`, `checks/two-control-probe.md`: `fix_shape = "edits"` is gone. A fixable defect "carries no `blocked_on`". The case that used to be needs-plan now carries `blocked_on = "design"` with `blocked_missing` and `blocked_contract`.

The detail fields match the requirements table in `tomlctl/src/review/fields.rs`: `decision` needs `blocked_missing` and `blocked_options`, and `design` needs `blocked_missing` and `blocked_contract`.

## Choices

- I used `decision` where the original sentence said the finding needs a decision or a choice about task boundaries (plan-executor, feasibility, plan-structure). I used `design` where it needs a new task, a new test or a cross-task contract change (the other six). In acceptance-contradiction, "both tasks genuinely need the contested state" could also be read as a decision. I chose `design` because the fix is a contract change across both tasks, not a pick between them.
- The plan-structure sentence grew a trailing clause naming the detail fields. That stays within the one sentence.

## Deviations

None. No `suggested` means was given.

## Checks

- Accept check `! grep -rqE 'needs-plan|fix_shape' claude/skills/review-v2-lenses`: pass.
- `tomlctl review catalogue` (installed binary) loads the catalogue without error.
- `cargo test ... cli::dispatch::tests` has Writes build: yes, so the gate runs it. I did not run it.
- No compile check: these are markdown-only edits.

## Handover

None.

TANGENTIAL: none
