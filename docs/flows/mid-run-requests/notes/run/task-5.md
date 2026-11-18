Status: done — plan request integration tests and output-options cases added and passing

```toml run-note
files_touched = ["tomlctl/tests/plan_request.rs", "tomlctl/tests/it.rs", "tomlctl/tests/output_options.rs"]

[[checks]]
command = "grep -q '^mod plan_request;' tomlctl/tests/it.rs && test -f tomlctl/tests/plan_request.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^plan_request::/)'"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^output_options::/)'"
result = "pass"
```

## What changed

- `tomlctl/tests/plan_request.rs` (new): 14 cases through the CLI covering every listed behaviour. Flows are seeded with `plan open`, the task store written with the sidecar helper, and an open `implement:A` run started with `plan run start --tasks`; plan.toml is never hand-written.
- `tomlctl/tests/it.rs`: `mod plan_request;` after `plan_refusals`.
- `tomlctl/tests/output_options.rs`: four cases (add, rule, list, applied) on three new plan-group flows (`request-add`, `request-open`, `request-ruled`) whose states are reached through the verbs.

The gate checks were run narrowly and passed.
