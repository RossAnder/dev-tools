Status: done — declaration table states requiredness and gains a `controls` key the review check reads, so review lenses skip planted-defect control states

```toml run-note
files_touched = [
  "claude/skills/ui-capture/SKILL.md",
  "claude/skills/ui-capture/references/defects.md",
  "claude/skills/review-v2-lenses/lenses/ui-visual.md",
  "claude/skills/review-v2-lenses/lenses/interaction.md",
  "claude/skills/review-v2-lenses/checks/ui-capture.md",
]

[[deviations]]
what = "The ui-visual control rule is a new `Controls.` probe bullet placed right after `Measured geometry.`, not text inside that bullet"
why = "The probes list is one bold-led bullet per concern; a separate bullet keeps the geometry rule readable and puts the skip next to it"
means = "fix text: add the sentence in ui-visual.md Rubric/Measured geometry"

[[tangential]]
kind = "bug"
area = ".claude/ui-capture.toml"
summary = "This repo's own declaration does not yet list its fixture controls under the new `controls` key, so a review of this repo still sees web-05 and the other planted controls as clean states until the orchestrator adds them"
why = "F72's failure scenario is this repo; the contract fix only takes effect once the declaration names the controls (the milestone's ui_controls give the list). Workers may not write under .claude/, so this needs an orchestrator store edit"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"

[[checks]]
command = "tomlctl review catalogue"
result = "pass"
```

## What changed

- `claude/skills/ui-capture/SKILL.md`: the declaration table now opens `command` and `out` with "Required." (`out` also says the visual pass builds each round's directory under it) and `paths` with "Optional.". A new optional `controls` key, shown in the example, lists the control states; a review reads it and the visual pass takes its controls from the milestone only. The Controls section now limits "not told" to the visual pass's inspector, which never reads `controls`. It says a review lens is told and skips the controls, and that a project lists every control under `controls`.
- `claude/skills/ui-capture/references/defects.md`: the Controls paragraph now limits "not told" to the visual pass's inspector and says a review lens is told and skips controls.
- `claude/skills/review-v2-lenses/lenses/ui-visual.md`: a `Controls.` probe after Measured geometry. A state the baseline lists under `controls:` is skipped: neither its plant nor its geometry entries is reported, and it is counted in the coverage note as a control.
- `claude/skills/review-v2-lenses/lenses/interaction.md`: the same rule appended to the Defect rubric probe.
- `claude/skills/review-v2-lenses/checks/ui-capture.md`: the contract key list names `controls`. The orchestrator also reads the declaration's `controls` array when the verification return arrives. Every baseline entry that names the directory gets `; controls: <state>, ...` appended when the list is non-empty. The finder paragraph says control states are skipped.

Picked option for F72 (from the task's constraints): a controls list in `.claude/ui-capture.toml`, read by the check. As that option warns, the inspector's blinding now depends on the prompt alone: the skill the inspector loads documents the key and tells it not to read it.
