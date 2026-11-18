Status: done — gate_list now appends each touched generator's check command, with tests

```toml run-note
files_touched = ["tomlctl/src/plan/emit.rs", "tomlctl/tests/plan_emit.rs"]

[[checks]]
command = "grep -q 'generated::' tomlctl/src/plan/emit.rs && grep -q 'generated:' tomlctl/tests/plan_emit.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "not-run"
```

## What changed

- `tomlctl/src/plan/emit.rs`: after the accept-check loop in `gate_list`, the milestone rows' files go through `generated::load` and `generated::touched`. Each touched generator's check is pushed with source `generated:<id>` through the existing `listed` set. A malformed config is a validation refusal naming `.claude/generated.toml`.
- `tomlctl/tests/plan_emit.rs`: three tests cover a touched generator (appended last), an untouched one (omitted) and one equal to a gate (not repeated). Not run.

Clippy failed on a sibling's in-flight file (`schema_request` not found in `super`), not on my files. Nextest is the gate's.
