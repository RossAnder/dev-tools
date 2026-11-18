Status: done — triage documents the escalation fix disposition, fix_files in Convert and audit, and agent-waves gains a One-row verify section

```toml run-note
files_touched = ["claude/skills/review-v2-engine/references/triage-and-close.md", "claude/skills/review-v2-engine/references/agent-waves.md"]

[[checks]]
command = "grep accept check for review escalation fix / fix_files / One-row verify"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

- triage-and-close.md: the audit rule names fix_files; the presentation lists open escalations; the grammar gains `fix C52:1` (`review escalation fix <cell>:<n> --ndjson <path>`, then the one-row verify); Convert allows a multi-file cleanup with instances and fix_files; the close summary names fixed escalations.
- agent-waves.md: new `## One-row verify` section after Mop-up.
