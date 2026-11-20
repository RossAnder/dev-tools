Status: done — collapsed the nested if in plan/drift.rs into a let-chain

```toml run-note
files_touched = ["tomlctl/src/plan/drift.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml (no drift.rs diagnostics)"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/drift.rs"
result = "pass"
```

## What changed
tomlctl/src/plan/drift.rs: the nested `if counted && ... { if let Some(..) = commits.last_mut() {..} }` is now one let-chain condition. Behaviour is unchanged. The workspace clippy accept check is the gate's.
