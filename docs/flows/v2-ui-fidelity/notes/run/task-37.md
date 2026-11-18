Status: done — inspector wording made control-blind; ui-capture skill states judges are not told

```toml run-note
files_touched = ["claude/agents/ui-inspector.md", "claude/skills/ui-capture/references/defects.md", "claude/skills/ui-capture/SKILL.md"]

[[checks]]
command = "! grep -qF '\"control\"' claude/agents/ui-inspector.md"
result = "pass"

[[checks]]
command = "control-flag absent from inspector and 'not told which' in defects.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

- ui-inspector.md: description, input list (per-state look numbers), look pass, pass verdict, Controls section, note first line and return shape rewritten per the detail.
- defects.md Controls: judge not told which states are controls.
- SKILL.md Controls: added the two sentences on blind controls and the `<surface>-<n>` naming.
