Status: done — generated.md written, both classes and the gate-list and lane-check text documented

```toml run-note
files_touched = ["claude/skills/tomlctl/references/generated.md", "claude/skills/tomlctl/references/plan.md", "claude/skills/tomlctl/references/plan-engine.md"]

[[tangential]]
kind = "bug"
area = "tomlctl/src/plan/check/generated_outputs.rs"
summary = "Findings are built with `class: CLASS` (a const), a shape the finding-class parser in finding_classes.rs does not read, so documented_plan_classes_match_the_source fails with the class documented but never emitted."
why = "The gate-covered test fails until the file states the class as a string literal, as case_collision.rs does."

[[checks]]
command = "test -f claude/skills/tomlctl/references/generated.md && grep -q 'plan/file-case-collision' plan.md && grep -q 'plan/generated-output-unheld' plan.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "fail"
```

## What changed

- generated.md (new): the file's purpose, a full example, key table, loader rules and the four readers.
- plan.md: two class-table rows and a pointer to plan-request.md.
- plan-engine.md: the `generated:<id>` source in gate-list and the lane-check sentence on granted, unapplied request paths.

The dispatch-tests check is gate-owned and ran only as extra. Its one failure comes from generated_outputs.rs (not my file): the class is written as a const, not a literal the test parser reads. The fix is to use the literal in that file's two `Finding` sites.
