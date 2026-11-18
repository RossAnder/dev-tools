Status: done — references/ui-layout.md is written: the UI/not-UI list, drafted options as HTML mocks rendered to PNG or as text grids, the Artifact page, the AskUserQuestion pick, the look decision, the ui-<milestone>.md design file, and the states, controls and look entries

```toml run-note
files_touched = ["claude/skills/plan-v2-engine/references/ui-layout.md"]

[[deviations]]
what = "The HTML mocks are written under the session scratchpad. Only their PNG renders, the render's geometry.json and terminal .txt grids go under docs/flows/<slug>/design/ui-<milestone>/"
why = "The detail names the PNG path and says nothing about where the mock HTML lives. The scratchpad is the engine's place for staging, and keeping the HTML out of design/ keeps what approval must commit to the pictures that workers and the inspector read. The other option was to commit each mock as <option>.html next to its PNG, which would make re-rendering possible but adds a file nobody reads."
means = "detail: an HTML mock per option, rendered to docs/flows/<slug>/design/ui-<milestone>/<option>.png"

[[deviations]]
what = "Added -narrow and -short extreme renders of each option, the fixed-surface stance-skip (a recorded design language or earlier pick means no options are drafted), and a row that declares .claude/ui-capture.toml when the project has no capture"
why = "Content and size extremes are where dead space shows, which the detail stresses. A recorded user stance already settles a question under Surface's third test. A milestone with ui_states and no capture command leaves the visual pass nothing to run."
means = "detail"

[[tangential]]
kind = "bug"
area = "tomlctl/src/cli/dispatch/tests/finding_classes.rs"
summary = "cli::dispatch::tests finding_classes fails: 'tomlctl/src/plan/schema.rs: no severity resolves for `class`'. The finding-class scanner reads the new decision `class` field as a finding class."
why = "The milestone gate fails this subset until the scanner, or the shape of the schema.rs site, is taught the difference"

[[checks]]
command = "grep -q 'Artifact' claude/skills/plan-v2-engine/references/ui-layout.md && grep -q 'render.sh' claude/skills/plan-v2-engine/references/ui-layout.md && grep -q '.txt' claude/skills/plan-v2-engine/references/ui-layout.md && grep -q 'ui_controls\\|--ui-control' claude/skills/plan-v2-engine/references/ui-layout.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "fail"
```

## What changed

`claude/skills/plan-v2-engine/references/ui-layout.md` (new) has five sections:

1. **Judge which milestones are UI.** A milestone counts as UI when it adds a surface or changes an arrangement; fixes do not count. The judgement is stack neutral: a change to the declared capture `paths` or to web/QML files makes a milestone a candidate, never the rule on its own (D26). The planner lists every milestone as UI or not UI and asks the user to correct the list (D5).
2. **Draft the options.** Two or three options per surface that really differ in arrangement, styled after the project's own tokens. Web and QML options are standalone HTML mocks rendered with the ui-capture `scripts/render.sh` into `design/ui-<milestone>/<option>.png`. The mock's geometry is read and fixed before the user sees anything, and a missing browser (exit 2) falls back to grids. Terminal options are text grids saved as `<option>.txt`, with no browser involved. When the Artifact tool is present (checked with ToolSearch), one Artifact page shows the options side by side (D4).
3. **Ask.** One AskUserQuestion per surface, each option carrying an ASCII preview or its grid, the recommended option first. A redrawn option is asked again.
4. **Record the pick.** The pick is a decision of class `look`, recorded with `surfaced = true` before asking, with `rejected` holding the other options, then answered by the open-and-challenge section 5 table and bound to the UI rows. `design/ui-<milestone>.md` gets a section per surface naming the chosen picture or grid and the constraints the pick implies. UI rows cite that section as `spec.design`. Every file the step writes is committed.
5. **Declare the states, controls and look entries.** Content and window extremes are declared as states with `--ui-state`, and at least one planted-defect control with `--ui-control`, which no look entry names (D28). `run_by = "look"` entries with `states` go on the UI rows, and an example shows the shape.

## Checks

The accept check passes. The `cli::dispatch::tests` subset, which CLAUDE.md names for `claude/**/*.md` edits, compiled and ran: 54 passed and 2 failed. Both failures are in `finding_classes` and name only `tomlctl/src/plan/schema.rs`, which is task 1's file and not one I hold (see the tangential entry). Every fenced `tomlctl` command in the new file uses single-token placeholders.
