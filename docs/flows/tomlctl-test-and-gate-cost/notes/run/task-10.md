Status: done — nine test files now import the shared Command builder

```toml run-note
files_touched = ["tomlctl/tests/integration.rs", "tomlctl/tests/items_dedupe.rs", "tomlctl/tests/items_dry_run.rs", "tomlctl/tests/json.rs", "tomlctl/tests/render_progress_log.rs", "tomlctl/tests/integrity_guards.rs", "tomlctl/tests/agent_contract.rs", "tomlctl/tests/blocks.rs", "tomlctl/tests/review_catalogue.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "accept: no file imports assert_cmd Command"
result = "pass"

[[checks]]
command = "accept: integration.rs still uses assert_cmd::cargo::cargo_bin"
result = "pass"
```

## What changed

In each of the nine files, `use assert_cmd::Command;` became `use crate::common::Command;`. Nothing else changed; `items_add_lock_contention_smoke` is untouched.
