Status: done — `inputs list --ledger` help now lists the `plan` ledger

```toml run-note
files_touched = ["tomlctl/src/cli/types/inputs.rs"]

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/cli/types/inputs.rs"
result = "pass"
```

## What changed

`tomlctl/src/cli/types/inputs.rs`: the `--ledger` help string on `InputsOp::List` is now `review|optimise|plan-review|backlog|plan`. No compile check was run, as the edit changes only a string literal. The reproduce-status accept check is the gate's.
