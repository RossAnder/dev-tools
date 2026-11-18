Status: done — report coverage section now lists ceiling overrides and escalations after Waivers

```toml run-note
files_touched = ["tomlctl/src/review/digest.rs", "tomlctl/tests/review_digest.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass"

[[checks]]
command = "grep -q 'Ceiling overrides' tomlctl/src/review/digest.rs && grep -q '## Escalations' tomlctl/src/review/digest.rs"
result = "pass"
```

## What changed

- digest.rs coverage(): after Waivers, `## Ceiling overrides` (from the run's `ceiling_overrides`) and `## Escalations` (from each cell's escalations); each omitted when empty.
- review_digest.rs: one test covering both present and both absent. The nextest check is the gate's and was not run.
