Status: done — five lens frontmatters carry the new slice rule or kinds, and the lenses skill documents them

```toml run-note
files_touched = ["claude/skills/review-v2-lenses/lenses/security.md", "claude/skills/review-v2-lenses/lenses/test-gaps.md", "claude/skills/review-v2-lenses/lenses/correctness.md", "claude/skills/review-v2-lenses/lenses/doc-drift.md", "claude/skills/review-v2-lenses/lenses/test-pruning.md", "claude/skills/review-v2-lenses/SKILL.md"]

[[checks]]
command = "grep accept check over the five frontmatters and SKILL.md"
result = "pass"
```

## What changed

- security.md: `slice = "signal-evidence"`; test-gaps.md: `slice = "changed-with-tests"`.
- correctness.md, doc-drift.md, test-pruning.md: added `kinds`.
- SKILL.md: slice field row lists the two new rules, a `kinds` row was added, and the `[routing]` paragraph names `lockfiles`.

The catalogue-validates test is the gate's to run and was not run here.
