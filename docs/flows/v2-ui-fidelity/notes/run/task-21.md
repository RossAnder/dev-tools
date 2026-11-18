Status: done — Surface asks hard-to-unwind and compromise calls with a budget, follow-up rounds and a class; Open interviews and reads the intent back

```toml run-note
files_touched = ["claude/skills/plan-v2-engine/SKILL.md", "claude/skills/plan-v2-engine/references/open-and-challenge.md"]

[[deviations]]
what = "Added the directions step as its own step-table row, 'Directions', between Surface and Design, rather than folding it into the Design row"
why = "The detail says the table gains the directions step before Design while task 23 writes it as the opening of write-and-prove.md section 1; a separate row keeps the order visible. The alternative, rewording the Design row, would hide that it runs every time. The row records no run step, so re-entry is unchanged"
means = "step table gains the directions step (before Design)"

[[tangential]]
kind = "bug"
area = "tomlctl/src/cli/dispatch/tests/finding_classes.rs"
summary = "documented_plan_classes_match_the_source and the_plan_scan_reads_both_construction_sites_and_no_tasks_class fail: 'tomlctl/src/plan/schema.rs: no severity resolves for `class`'. The finding-class scanner reads the new `vocab(table, \"class\", ...)` call in the decision parser as a finding class"
why = "The milestone's dispatch-tests gate fails until the scanner is taught that shape or the call is reworded; no task in this milestone names that test file as far as this worker saw"

[[checks]]
command = "accept check (grep for compromise, eight, outside the budget, class, lift, see and do in open-and-challenge.md; no 'zero questions before the gate is the normal case' in SKILL.md)"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "fail"
```

## What changed

- `claude/skills/plan-v2-engine/SKILL.md`: the dialogue role rule now asks every ambiguous call that is hard to unwind or a compromise, at any level, taste and UX included, with follow-ups asked and never assumed, and the aim of building better than specified while staying consistent with the user's expectations. The step table's Open row names the interview and read-back; a new Directions row, pointing at write-and-prove.md, sits before Design; the Surface and Surface again rows no longer call zero questions normal. The open-run rule names `visual:` beside `implement:` and `reproduce:`. The references list gains `references/ui-layout.md` and mentions the interview, read-back and directions step. "the three references" became "its references", since the skill already had four.
- `claude/skills/plan-v2-engine/references/open-and-challenge.md`: section 1 gains the interview (in `AskUserQuestion` rounds of up to four questions, scaled to complexity and ambiguity) before the brief, and the read-back of what the user will see and do after it, with corrections folded in before Investigate and a pointer to the second read-back in write-and-prove.md. In section 5, test 1 is the new visible, costly or compromise test, and candidates include look, behaviour, cost and naming calls. An agent-assumed one-way door is always surfaced. The budget is eight in the first round, in batches of up to four, with follow-up rounds outside the budget until none opens, and one line cites the surfacing risk in `docs/plans/v2-planning.md`. "Zero survivors is the normal case" is gone, and recording nothing surviving stays. A question narrowed by an engineering limit states what lifting it would cost. Surfaced and unsurfaced decisions carry a `class` from the vocabulary in `tomlctl/src/plan/schema.rs` (`DecisionClass`), and the record example shows it. Section 6 names `visual:` runs.

## Checks

The accept check passes. The dispatch-tests subset had 3 failures, none caused by these edits. `skill_markdown_links_resolve` flags the new link to `references/ui-layout.md`, which sibling task 22 creates, so it resolves once that file lands. The two finding-class failures name `tomlctl/src/plan/schema.rs`, which this task does not hold. See the tangential entry.
