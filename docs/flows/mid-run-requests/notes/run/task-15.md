Status: done — needs_human report moved to references/report.md, links repointed

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/drain.md", "claude/skills/implement-v2-engine/references/report.md", "claude/skills/implement-v2-engine/SKILL.md", "claude/skills/implement-v2-engine/references/questions.md"]

[[checks]]
command = "test -f report.md && grep -q '### Record the report' report.md && ! grep -q '### Record the report' drain.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed

- drain.md: section 11's body (594 to 513 lines) moved verbatim to report.md; the heading stays with one sentence pointing at report.md. Mentions of step 11 for the report now link report.md.
- report.md: new, with a one-line intro and the moved text; the bare step 6 and step 10 citations now name drain.md.
- SKILL.md: step-table row links report.md; references list gains a report.md bullet.
- questions.md: the needs_human pointer links report.md.

The dispatch-test gate was left to the gate, as marked. Files keep their CRLF endings.
