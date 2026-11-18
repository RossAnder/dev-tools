Status: done — the ui-capture skill states the stack-neutral capture contract, and its references/defects.md holds the ten-class visual defect rubric

```toml run-note
files_touched = ["claude/skills/ui-capture/SKILL.md", "claude/skills/ui-capture/references/defects.md"]

[[checks]]
command = "grep -q -- '--list' claude/skills/ui-capture/SKILL.md && grep -q 'geometry.json' claude/skills/ui-capture/SKILL.md && grep -q 'ui-capture.toml' claude/skills/ui-capture/SKILL.md && grep -qi 'ratatui' claude/skills/ui-capture/SKILL.md && grep -qi 'qml' claude/skills/ui-capture/SKILL.md"
result = "pass"

[[checks]]
command = "grep -qi 'empty space' claude/skills/ui-capture/references/defects.md && grep -qi 'scal' claude/skills/ui-capture/references/defects.md && grep -qi 'column' claude/skills/ui-capture/references/defects.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed

- `claude/skills/ui-capture/SKILL.md` (new). It covers the `.claude/ui-capture.toml` declaration (`command`, `out`, `paths`), the command's flags (`--out`, repeated `--only`, `--list`) and what exit 0 means. It defines the outputs: `<state>.png` at a fixed size and scale 1, never full-page, and `<state>.txt`, a cell grid with no escape codes. It gives the `geometry.json` shape (an object keyed by state, each value an array of `{kind, locator, detail}`, where kind is overflow, clipping, overlap or offscreen). Every image state needs a key, and a missing key is a capture failure. Text-grid-only states are exempt. It has one paragraph each for web, QML/Quickshell, terminal UIs (ratatui `TestBackend`) and other stacks, then sections on controls and the three judging rules, and a references table that points at task 14's `scripts/geometry-probe.js` and `scripts/render.sh`. The contract needs no browser and no web file type.
- `claude/skills/ui-capture/references/defects.md` (new). It holds ten defect classes. Each is a yes-or-no question with what counts, what does not, and worked examples for web, QML and terminal. It also states the answer rules: an observed defect is a fail and is never talked down, an unsure answer is reported as unsure, and measured geometry outranks the eye. It says to judge against the mock as a separate pass and gives guidance for planted controls.

## Checks

Both accept checks pass. `cli::dispatch::tests`, which CLAUDE.md names for `claude/**/*.md` edits, did not compile. The only errors are E0063 missing fields (`ui_states`/`ui_controls` on `MilestoneSpec` at `tomlctl/src/plan/milestone.rs`, and `class` on `Decision` at `tomlctl/src/plan/records.rs`), and both of those are sibling tasks' in-flight edits. My files contain no fenced `tomlctl` commands, so the placeholder rule does not apply to them. The gate runs this subset once the milestone is complete.

## Judgement calls

- The detail did not say whether the judge is told which states are controls. Task 15's C2 return shape carries a `control` field per state, so the text says that a control must come back with at least one `yes`, and that a judge finding none in it says so. It does not say the controls are hidden from the judge.
- For a state captured as both an image and a text grid, the image rule governs: its `geometry.json` key is owed. That follows D33, which exempts only text-grid states.
