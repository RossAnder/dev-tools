Status: done — launch-args drops a row whose outside need is done but unstamped

```toml run-note
files_touched = ["tomlctl/src/plan/emit.rs", "tomlctl/tests/plan_emit.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass"

[[checks]]
command = "grep -qF 'need-not-committed' tomlctl/src/plan/emit.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^plan_emit::launch_args/)'"
result = "pass"
```

## What changed

- `tomlctl/src/plan/emit.rs`: `stranded` now treats a need whose row is in another milestone as met only when done with a non-empty commit, else `need-not-committed:<need>`. Needs inside the launched milestone are judged as before.
- `tomlctl/tests/plan_emit.rs`: new test `launch_args_need_a_commit_stamp_on_a_need_outside_the_milestone` covering the unstamped (dropped) and stamped (launches) cases.
