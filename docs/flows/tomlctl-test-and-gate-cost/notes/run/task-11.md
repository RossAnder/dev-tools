Status: done — ten flow test files now use the shared command builder

```toml run-note
files_touched = ["tomlctl/tests/flow_stale.rs", "tomlctl/tests/flow_active.rs", "tomlctl/tests/flow_envelope.rs", "tomlctl/tests/flow_resolve.rs", "tomlctl/tests/flow_list.rs", "tomlctl/tests/flow_init.rs", "tomlctl/tests/flow_find_plans.rs", "tomlctl/tests/flow_ensure_artifact.rs", "tomlctl/tests/flow_doctor.rs", "tomlctl/tests/flow_path_seam.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "accept: no assert_cmd Command import; tomlctl_process in flow_active.rs"
result = "pass"
```

## What changed

Each file's `use assert_cmd::Command;` became `use crate::common::Command;`. In flow_active.rs the concurrent add test builds its commands with `common::tomlctl_process()` so it keeps real processes.
