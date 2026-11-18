Status: done — measurement reads audit_classes and --yield; plan-review-lead reserves 1 and never overrides the ceiling

```toml run-note
files_touched = ["claude/skills/review-v2-engine/references/measurement.md", "claude/agents/plan-review-lead.md"]

[[checks]]
command = "grep accept check (--yield, audit_classes, --reserve 1, over-ceiling-file)"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

- measurement.md: Audit overturns row names audit_classes; new Yield row for `review stats --yield`; the by-hand class counting now reads audit_classes from `review stats --all --by model`.
- plan-review-lead.md: proposal step passes `--reserve 1` at quick and recheck and never `--over-ceiling-file`; follow-up step says to cut a candidate or repeat cell on a ceiling refusal, or return.
