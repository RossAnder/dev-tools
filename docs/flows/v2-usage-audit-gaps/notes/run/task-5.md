Status: done — worker-owned building accept checks are now accepted

```toml run-note
files_touched = ["tomlctl/src/tasks/schema.rs", "tomlctl/tests/tasks_spec_add.rs", "claude/skills/tomlctl/references/tasks-store.md"]

[[checks]]
command = "! grep -q 'self.run_by == RUN_BY_WORKER && self.writes_build' tomlctl/src/tasks/schema.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --tests"
result = "fail"
```

## What changed

- schema.rs: removed the owner_problem branch refusing run_by worker with writes_build true.
- tasks_spec_add.rs: the refusal test became `accept_worker_owner_accepts_a_building_check`, asserting the row is stored and shown with run_by worker and writes_build true; the other refusals are kept.
- tasks-store.md: the worker row and the refusal list now say a worker-owned check may build, the worker runs it, gate-list skips it and probes skip a building one.

The clippy failure names only sibling files (plan/approve.rs, plan/emit.rs, plan/run_gate.rs: in-flight edits from other tasks). The nextest accept check is the gate's and was not run.
