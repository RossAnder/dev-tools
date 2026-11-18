Status: done — holders module with fold_key, forbidden and the D6 holder test, declared in plan/mod.rs

```toml run-note
files_touched = ["tomlctl/src/plan/holders.rs", "tomlctl/src/plan/mod.rs"]

[[deviations]]
what = "The needs closure up and down is a local walk over needs and coupling edges in holders.rs, not Graph::build with closure_up and closure_down"
why = "crate::tasks::graph is a private module (`mod graph;` in tomlctl/src/tasks/mod.rs, which re-exports only cycle_within and layered_kahn), so plan/ cannot reach Graph without editing tasks/mod.rs, which this task does not hold. plan/emit.rs drop_dependents already walks the same needs and coupling edges locally for the same reason. Alternatives weighed: fail with needs_files naming tasks/mod.rs (costs a replan round for a one-line re-export), or the local walk (same edges, same result on an acyclic store; a dangling edge is skipped and a cycle ends the walk instead of an error, which tasks check reports anyway)."
means = "detail: Graph::build over crate::tasks nodes, closure_up and closure_down, as tasks/closure.rs builds it"

[[deviations]]
what = "holder() refuses a requester absent from the task store with a Validation-tagged error; forbidden() also treats a drive-letter prefix as absolute"
why = "The detail does not say what an unknown requester returns; a refusal matches how the task verbs report a missing id. Windows paths such as C:/x fold to a drive prefix that normal_path keeps."
means = ""

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/holders.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "grep -q 'pub(crate) fn fold_key' tomlctl/src/plan/holders.rs && grep -q 'mod holders' tomlctl/src/plan/mod.rs"
result = "pass"

[[checks]]
command = "test \"$(grep -c '#\\[test\\]' tomlctl/src/plan/holders.rs)\" -ge 5"
result = "pass"
```

## What changed

- `tomlctl/src/plan/holders.rs` (new): `fold_key` (normal_path then lowercase), `forbidden(slug, path)` (absolute, `..` segment, under `.claude/` or `docs/flows/<slug>/`, all by folded key with a directory-boundary match so `docs/flows/<slug>-two/` and `.claudette/` pass), `Holder { task, status, grant }` and `holder(tasks, plan, requester, path)` implementing D6: candidates are rows of the requester's checkpoint, status pending, in-progress, failed or parked, outside the requester's needs-and-coupling closure up and down; a candidate holds by a folded file match or by a granted path item of one of its unapplied requests (grant = request id). Lowest id wins. Eleven unit tests cover the case-variant hold, failed and parked holders with lowest-id order, dependent and dependency exclusion, done and deferred exclusion, another milestone, an unapplied grant holding, applied, denied and open grants not holding, an unknown requester, and the forbidden paths.
- `tomlctl/src/plan/mod.rs`: `mod holders;` (private; plan's descendants, such as the request verbs and plan/check's case-collision check, can reach it as `super::holders` / `crate::plan::holders` from inside plan).

Clippy compiled the lib and tests with no errors; it warns that the holders items are unused, as it does for schema_request's `mint_request_id`, until the request verbs and the case-collision check that consume them land in the same flow. The unit tests themselves (`plan::holders::`) are the gate's to run.
