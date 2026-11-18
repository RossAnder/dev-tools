Status: done — route() reads fix_files; executable fix_files send a cleanup to the verifier

```toml run-note
files_touched = ["tomlctl/src/review/fields.rs", "tomlctl/src/review/add.rs", "tomlctl/tests/review_add.rs"]

[[checks]]
command = "grep -A12 'pub(crate) fn route(' tomlctl/src/review/fields.rs | grep -q 'fix_files'"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass"
```

## What changed

- fields.rs: `route` takes `fix_files: &[String]` and checks each like an edited path in the cleanup arm; unit-test calls updated.
- add.rs: `route_of` reads the row's `fix_files` and passes it.
- review_add.rs: new test `cleanup_fix_files_with_executable_code_route_to_verifier` (verifier with .rs fix_files, checker without). The gate runs the nextest.

The spec's convert test (`review triage --convert cleanup` with fix_files) belongs in review_triage.rs, which is outside this task's files, so it was not added. Triage mints through `add::route_of`, so it routes by fix_files too.
