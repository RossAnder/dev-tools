Status: done — look owner and milestone ui fields documented in both task-store references

```toml run-note
files_touched = ["claude/skills/tomlctl/references/tasks-store.md", "claude/skills/tomlctl/references/tasks-write.md"]

[[checks]]
command = "grep -q 'run_by = \"look\"' tasks-write.md && grep -q 'ui_controls' tasks-store.md"
result = "pass"
```

## What changed

- tasks-store.md: added a `look` row to the accept owner table with the `states` rules, and `ui_states` / `ui_controls` to the milestone shape.
- tasks-write.md: the `spec` accept description now covers `run_by = "look"` with `states` and its refusals.
