Status: done — the ui-inspector agent judges captured states in four separate passes and returns the C2 shape

```toml run-note
files_touched = ["claude/agents/ui-inspector.md"]

[[deviations]]
what = "Added `skills: [ui-capture]` and `color: magenta` to the frontmatter beyond the listed name, description, model, effort and tools"
why = "review-verifier.md, the named frontmatter precedent, carries `color:` and a `skills:` preload; preloading ui-capture puts its judging rules in context, which the tools list (no Skill tool) could not otherwise load. The link scripts already junction skills/ui-capture into the config dir. The alternative, naming the rubric path only in the prompt, leaves the binding rules unloaded when the Workflow omits it"
means = "Frontmatter: name, description, model opus, effort high, tools Read, Grep, Glob, Bash, Write"

[[deviations]]
what = "Each state's `note` field in the return carries the note path, and unsure answers are kept out of `defects` and written to the note with their location"
why = "The C2 shape does not say whether `note` is a path or a summary; the Workflow task builds an index with each state's note path, so the path is what it consumes. Keeping `defects` to observed defects lets visual.md mint one fix task per entry without filtering unsure ones"
means = "Return the C2 shape: {states: [{state, control, verdict, defects: [{class, location, detail}], note}]}"

[[checks]]
command = "grep -q 'unsure' claude/agents/ui-inspector.md && grep -qi 'control' claude/agents/ui-inspector.md && grep -q 'defects.md' claude/agents/ui-inspector.md && grep -q '^model:' claude/agents/ui-inspector.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "fail"
```

## What changed

`claude/agents/ui-inspector.md` (new): frontmatter after review-verifier.md (opus, effort high, tools Read, Grep, Glob, Bash, Write, ui-capture preloaded). The body names the prompt's inputs (slug, milestone, round, capture_dir, design, states with control flags, looks, note path), locates `references/defects.md` (prompt path, then the config dir's skill, then the repo copy), and runs per state: a capture-presence check (a missing output or a missing geometry key for an image state is `fail` with class `capture`, per D33), then four separate passes: geometry entries as defects with their locators, every rubric class answered yes/no/unsure with a location and a re-read before a yes, each look entry naming the state judged met/not met/unsure, and a design comparison on arrangement, not pixels. Verdict rules: any observed defect is `fail` and is never downgraded, unsure is never pass, and pass needs every answer clean. Control rule: judged like any other state, never given an invented defect, and a control showing no defect is stated in the note's first line. Defect class names are short slugs of the rubric's ten classes plus `look`, `design` and `capture`. Write is used only for the note; Bash is read-only and scratch crops go in the session scratchpad.

No teammate-delivery shared block is carried: `scripts/shared-blocks.toml`'s `research-delivering` list does not include every agent (verification.md, flow-bootstrap.md and the plan-worker pair are absent), and the verifier checks only listed files, so the constraint's "otherwise none" applies.

## Checks

The accept check passes. The dispatch subset ran 54 pass and 2 fail: `finding_classes::documented_plan_classes_match_the_source` and `the_plan_scan_reads_both_construction_sites_and_no_tasks_class`, both reporting `tomlctl/src/plan/schema.rs: no severity resolves for class`. That file belongs to a sibling task's in-flight edit, not this task; command_lint and the agent-name gates passed.
