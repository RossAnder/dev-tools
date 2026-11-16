Status: done — SKILL.md now describes the dependency pick

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/SKILL.md"]

[[checks]]
command = "grep accept checks (schedule + --elaborate present; earlier-checkpoint refusal gone)"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

`claude/skills/implement-v2-engine/SKILL.md`: intro and loop paragraph continue while `plan emit schedule` names a pick; `--milestone` is taken when runnable or the pick and refused naming its state otherwise; deferred milestone is skipped, reported and elaborated by `/plan-v2 <slug> --elaborate <id>`; re-entry settles after the stage check with the scoped `plan check --milestone <id>` after the pick; open `elaborate:`/`cold-read:` runs are left alone; unstamped milestones use the schedule's `unstamped` state; parked rows of every `parked` milestone are re-asked before the pick, and a left-parked row holds its milestone and downstream ones. File is under 500 lines.
