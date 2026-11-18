Status: done — compose, the review-v2 carrier and the engine skill now describe the new proposal flags, ceiling override and escalation recording

```toml run-note
files_touched = ["claude/skills/review-v2-engine/references/compose.md", "claude/commands/review-v2.md", "claude/skills/review-v2-engine/SKILL.md"]

[[checks]]
command = "grep accept check (over-ceiling-file, review escalation record, --context-free, review_plan_ceiling)"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed
- compose.md: proposal paragraph (phases, kind-aware slices, pool, repeats hotspot first, --context-free, --reserve, over_cap), re-split guidance, ceiling refusal and --over-ceiling-file, family deltas, escalation record verb and a fourth disposition, quick escalations (no probe for code and perf), reserve.
- review-v2.md: context-free section now uses --propose --context-free and --reserve; appended row removed; stats pointer to --yield.
- SKILL.md: pre-flight features, ceiling and repeat rows, quick probe line. Two table rows were edited with sed -i (small LF-only diff).
- The fix disposition's command lives in triage-and-close.md, which this task does not hold; compose.md only points to it.
