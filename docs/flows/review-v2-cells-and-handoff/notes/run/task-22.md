Status: done — review.md documents the new review plan flags, escalation verbs, stats classes and yield, and the ceiling and escalation fields

```toml run-note
files_touched = ["claude/skills/tomlctl/references/review.md"]

[[checks]]
command = "grep accept check over review.md (review escalation, --over-ceiling-file, --yield, ceiling_overrides, --context-free)"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

`claude/skills/tomlctl/references/review.md`: store shape gains the run's `ceiling_overrides` and the cell's `escalations`, with a computed-fields paragraph in the field catalogue. Routing counts `fix_files`. `review plan` documents `--context-free`, `--reserve`, `--over-ceiling-file`, the summary keys `reserve`, `mechanical_pool` and `context_free`, the three admission phases, `over_cap`'s meaning, kind-aware slicing and the `signal-evidence` and `changed-with-tests` rules, the refusal and the envelope's `ceiling` and `ceiling_override`. New `review escalation record` and `review escalation fix` sections, one table each so the flag-table gate attributes flags correctly. `review stats` documents `audit_classes`, the run rows' `ceiling_overrides`, and `--yield` with its shape and arm and uniqueness rules.

The gate (which the spec marks as the gate's own check) was also run once and passes.
