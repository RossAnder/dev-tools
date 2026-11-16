Status: done — schedule leaf tests and output_options case added

```toml run-note
files_touched = ["tomlctl/tests/plan_emit.rs", "tomlctl/tests/output_options.rs"]

[[checks]]
command = "grep -qF '\"schedule\"' tomlctl/tests/plan_emit.rs"
result = "pass"

[[checks]]
command = "grep -A1 -F '\"emit\",' tomlctl/tests/output_options.rs | grep -qF '\"schedule\"'"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^plan_emit::schedule/) | test(/^output_options::/)'"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/plan_emit.rs tomlctl/tests/output_options.rs"
result = "pass after applying the one reported diff"
```

## What changed

- `tomlctl/tests/plan_emit.rs`: `schedule_picks_the_runnable_milestone_and_reports_why_the_rest_wait` covers pick C, B deferred, D waiting on B and C, B elaborating under an open `elaborate:B` run, and C waiting with an empty pick after `plan milestone update --id C --after B`.
- `tomlctl/tests/output_options.rs`: a `plan emit schedule` read case expecting `pick`.
