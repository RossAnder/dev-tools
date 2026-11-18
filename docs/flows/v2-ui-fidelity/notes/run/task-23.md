Status: done — Design opens with directions, direction.md and the second read-back, runs the UI layout step, Surface again cites Surface's rule, and the spec rules require direction citations and look entries

```toml run-note
files_touched = ["claude/skills/plan-v2-engine/references/write-and-prove.md"]

[[tangential]]
kind = "bug"
area = "tomlctl/src/plan/schema.rs"
summary = "The finding-class scan in cli::dispatch::tests::finding_classes panics on schema.rs: 'no severity resolves for `class`'. The new decision `class` field's validation reads to the scanner as a finding-class construction site."
why = "Two dispatch tests (documented_plan_classes_match_the_source, the_plan_scan_reads_both_construction_sites_and_no_tasks_class) fail, and the pre-commit hook runs that subset, so the milestone cannot commit until either schema.rs or the scanner is changed."

[[checks]]
command = "accept check: grep for ui-layout.md, direction.md, run_by = \"look\", read-back, and no 'It must be a different plan'"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "fail"
```

## What changed

`claude/skills/plan-v2-engine/references/write-and-prove.md`:

- Section 1, Design, now opens with **Directions**. It drafts two or three distinct directions with their trade-offs (cost, risk, what each makes easy or hard later), scaled to the work's complexity and ambiguity, and asks the user to choose or steer. The choice is recorded as a surfaced decision of class `architecture`, and the answer is applied through open-and-challenge.md section 5's table. Then it writes `design/direction.md`: an `## Overarching` section, plus one `## Milestone <id>` section per milestone once Milestones cuts them.
- The **second read-back** follows: the account made concrete by the investigation and the chosen direction, with changes marked. Corrections go into the brief, and into direction.md when they touch the direction.
- **Decisions** now carry a `class`, and the worked example shows `class = "architecture"`.
- **UI milestones**, placed after the milestone-order and defer paragraphs, routes a milestone that adds or reshapes a UI surface through `ui-layout.md`. States and controls are declared with `--ui-state` and `--ui-control`, on `milestone add` or `milestone update`. **Design files** names direction.md and ui-<milestone>.md.
- **Handovers**: a milestone with `ui_states` records no look-review or visual-repro handover. The check goes into a look entry instead.
- Section 2, **Surface again**: the three restated tests are gone, along with "This normally finds nothing". The section now cites open-and-challenge.md section 5 for the tests, the per-session budget (which counts Surface's own questions) and the follow-up rule, and uses that section's item shape, class and answer table.
- Section 3: the worked example now has `design = ["design/direction.md#milestone-a"]` on both rows. Its read-only entries are reworded as internal facts. A new paragraph after the example says the `test -f` and `grep` entries prove internal facts, and that the one user-facing behaviour is accepted by the test that drives it.
- New spec rules: (a) every task cites its direction.md section in `design`, and a UI row also cites its ui-<milestone>.md section; (b) a user-visible behaviour is never accepted on a read-only check alone. It gets a test that drives it, or a look entry (`run_by = "look"`, `states` drawn from `ui_states` and never a control, empty `check`). An example is included.

## Choices

- Not counting the directions question against Surface's eight is a choice the plan left open. The other option was to count it. I chose not to because the directions step is mandated for every plan (D31), and ui-layout.md likewise asks its layout questions outside Surface. Counting it would leave Surface seven questions on every plan. This is not a departure from a suggested means.
- direction.md's per-milestone sections are written after Milestones, because milestones do not exist when the direction is chosen.

## Checks

- The accept check passes.
- `cli::dispatch::tests` fails on three tests, none of which reads this file. `catalogue_validates` fails on concerns with no owner in the review-v2-lenses catalogue (vocabulary.toml and the lens files are siblings' in-flight edits). The two `finding_classes` tests panic on `tomlctl/src/plan/schema.rs`, a file this task does not hold. That is recorded as a tangential bug above.
