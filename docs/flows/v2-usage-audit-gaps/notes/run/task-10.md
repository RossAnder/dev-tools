Status: done — a task-id bind now also reaches rows sharing a file with the bound task

```toml run-note
files_touched = ["tomlctl/src/plan/view.rs", "tomlctl/tests/plan_view.rs"]

[[checks]]
command = "grep -q 'fn a_task_bind_reaches_a_row_on_the_bound_tasks_files' tomlctl/tests/plan_view.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "fail"
```

## What changed
- view.rs: `bind_names` takes the store; an all-digit bind matches the row with that id or any row sharing a (normalised) file with the bound task.
- plan_view.rs: added `a_task_bind_reaches_a_row_on_the_bound_tasks_files` (adds a fix row 3 on src/a.rs).

Clippy fails only on E0027 errors in approve.rs, emit.rs and run_gate.rs, siblings' in-flight edits; nothing names view.rs. The nextest check is the gate's.
