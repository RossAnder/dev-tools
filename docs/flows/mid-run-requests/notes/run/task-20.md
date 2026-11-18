Status: done — write-and-prove now carries the generator rule and handles both new warnings

```toml run-note
files_touched = ["claude/skills/plan-v2-engine/references/write-and-prove.md"]

[[checks]]
command = "grep for plan/generated-output-unheld, plan/file-case-collision, generated.toml in write-and-prove.md"
result = "pass"

[[checks]]
command = "cargo test --lib -- cli::dispatch::tests"
result = "fail"
```

## What changed

`write-and-prove.md`: added a spec-rule bullet (a row whose files match a generated.toml input holds each output and names the regenerate command, unless a dependent holds and regenerates), and extended the 4a paragraph with handling for both warnings. File is about 428 lines, under the 600 reference ceiling.

The dispatch tests failed only on `documented_plan_classes_match_the_source`, naming generated_outputs.rs and plan.md (no table row for plan/file-case-collision), which are other tasks' in-flight files, not this one's.
