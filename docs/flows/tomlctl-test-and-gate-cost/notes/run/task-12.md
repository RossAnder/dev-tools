Status: done — nine plan test files now import the shared Command builder

```toml run-note
files_touched = ["tomlctl/tests/plan_check.rs", "tomlctl/tests/plan_stage.rs", "tomlctl/tests/plan_records.rs", "tomlctl/tests/plan_view.rs", "tomlctl/tests/plan_refusals.rs", "tomlctl/tests/plan_lifecycle.rs", "tomlctl/tests/plan_drift.rs", "tomlctl/tests/plan_approve.rs", "tomlctl/tests/plan_open.rs"]

[[checks]]
command = "! grep -l 'use assert_cmd::Command;' <nine files> | grep -q ."
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass (no errors or warnings naming the nine files)"
```

## What changed

In each of the nine files, `use assert_cmd::Command;` became `use crate::common::Command;`. All nine already imported `crate::common`, so no other import was added. No assertions changed.
