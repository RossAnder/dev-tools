Status: done — worker view prints a Generators section per touched generator

```toml run-note
files_touched = ["tomlctl/src/plan/view.rs", "tomlctl/tests/plan_view.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass"

[[checks]]
command = "grep -q 'Generators' tomlctl/src/plan/view.rs"
result = "pass"

[[checks]]
command = "grep -q 'generated.toml' tomlctl/tests/plan_view.rs"
result = "pass"
```

## What changed

- `view.rs`: `generator_lines` prints one bullet per generator the task's files touch (outputs, regenerate command, not-held note); a malformed config prints one line naming the problem.
- `plan_view.rs`: three tests (touched, untouched, malformed). Not run by me; the nextest accept check is the gate's.
