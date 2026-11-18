Status: done — the lane check now counts granted, unapplied request paths as in lane

```toml run-note
files_touched = ["tomlctl/src/plan/lane.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "grep -q 'granted' tomlctl/src/plan/lane.rs && grep -q 'requests' tomlctl/src/plan/lane.rs"
result = "pass"

[[checks]]
command = "grep -A1 '#[test]' tomlctl/src/plan/lane.rs | grep -q 'fn [a-z_0-9]*grant'"
result = "pass"
```

## What changed

tomlctl/src/plan/lane.rs: `out_of_lane` takes the plan's requests and builds its lane set through a new `lane_paths` helper, which adds each path item ruled granted on an unapplied request of a row in the lane. `lane_check` passes `plan.requests`. Doc comment updated. Two unit tests cover granted (in lane) and denied, applied or foreign-task (not in lane). The remaining clippy warnings (unused `ID_PREFIX`, `mint_request_id`) belong to another task's file. The nextest accept check is the gate's.
