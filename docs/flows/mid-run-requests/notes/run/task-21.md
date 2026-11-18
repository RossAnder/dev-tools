Status: done — both findings in generated_outputs.rs now state the class as a literal

```toml run-note
files_touched = ["tomlctl/src/plan/check/generated_outputs.rs"]

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/check/generated_outputs.rs"
result = "pass"
```

## What changed
generated_outputs.rs: replaced both `class: CLASS` with the literal "plan/generated-output-unheld" and removed the unused const. The catalogue test is a gate check and was not run.
