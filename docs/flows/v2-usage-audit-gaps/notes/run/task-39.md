Status: done — report.md's withheld: section now lists the handover-withheld line

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/report.md"]

[[checks]]
command = "grep -q 'handover <id> not done' claude/skills/implement-v2-engine/references/report.md"
result = "pass"
```

## What changed

`claude/skills/implement-v2-engine/references/report.md`: added `- task <n>: handover <id> not done — <action>` under `withheld:`, directly after the premise line, matching the text run-milestone.md names. I did not run the `cli::dispatch::tests` subset. It builds under the shared lock, and the edit is one line inside a plain text block.
