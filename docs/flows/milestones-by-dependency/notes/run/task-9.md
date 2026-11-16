Status: done — after approval, plan stage lists elaborate/cold-read runs in open_runs without making them blockers

```toml run-note
files_touched = ["tomlctl/src/plan/stage.rs", "tomlctl/tests/plan_stage.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^plan_stage::/)'"
result = "pass"

[[checks]]
command = "grep -qF 'elaborate:' tomlctl/src/plan/stage.rs"
result = "pass"
```

## What changed

- `tomlctl/src/plan/stage.rs`: `run` filters open runs whose step starts with `elaborate:` or `cold-read:` out of the set passed to `with_open_runs` once the plan has an approval. All open steps still print in `open_runs`. Before approval nothing changes.
- `tomlctl/tests/plan_stage.rs`: three tests (approved elaborate/cold-read listed but not blocking; approved implement run still blocks; unapproved cold-read still blocks).
