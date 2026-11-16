Status: done — added plan/milestone-order-unqualified to both finding-class lists in the lifecycle test

```toml run-note
files_touched = ["tomlctl/tests/plan_lifecycle.rs"]

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/plan_lifecycle.rs"
result = "pass"
```

## What changed

`tomlctl/tests/plan_lifecycle.rs`: both `finding_classes` expectations (the two `plan check` calls) now end with `plan/milestone-order-unqualified`, after `plan/milestone-no-gate`, matching the sorted order. The fixture keeps milestone B unqualified (no `--after`) in both places. The lifecycle test itself is the gate's to run (it builds), so it was not run here.
