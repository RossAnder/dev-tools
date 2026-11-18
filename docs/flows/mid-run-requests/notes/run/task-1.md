Status: done — generated module added and declared in lib.rs

```toml run-note
files_touched = ["tomlctl/src/generated.rs", "tomlctl/src/lib.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --lib"
result = "pass"

[[checks]]
command = "grep -q '^mod generated;' tomlctl/src/lib.rs && grep -q 'pub(crate) fn outputs_of' tomlctl/src/generated.rs"
result = "pass"

[[checks]]
command = "test \"$(grep -c '#\\[test\\]' tomlctl/src/generated.rs)\" -ge 3"
result = "pass"
```

## What changed

- `tomlctl/src/generated.rs`: new. `load`, `touched`, `outputs_of`, `Generator`, with strict validation (missing or blank key, duplicate id, unknown key, glob output, all naming the generator) and case-insensitive input globs. Four unit tests. The module carries `#![allow(dead_code)]` because its callers land in later tasks.
- `tomlctl/src/lib.rs`: `mod generated;`.

The nextest accept check is the gate's and was not run.
