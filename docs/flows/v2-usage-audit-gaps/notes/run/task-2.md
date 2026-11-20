Status: done — milestone A flags added to the plan CLI types

```toml run-note
files_touched = ["tomlctl/src/cli/types/plan.rs"]

[[checks]]
command = "grep checks for after-round, waive-review-delta, baseline-gate, baseline-proof, base"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "fail"
```

## What changed

`tomlctl/src/cli/types/plan.rs`:
- `PlanRunGateArgs`: `accept` joins the `gate_rows` group. New `--base` (conflicts with `--rerun`), `--accept` (conflicts with `--rerun` and `--base`) and `--json <SRC>` (requires `--accept`).
- `PlanEmitGateListArgs`: `--after-round <N>` (`Option<u32>`).
- `PlanMilestoneAddArgs`: `--baseline-gate` (repeatable) and `--ui <yes|no>`.
- `PlanMilestoneUpdateArgs`: `--baseline-gate` and `--baseline-proof` (both repeatable) and `--ui <yes|no>`.
- `PlanApproveArgs`: `--waive-review-delta <WHY>`.

## Compile check

Clippy fails with E0027 (missing fields in exhaustive destructuring of the args). All three sites are in sibling-held files:
- `tomlctl/src/plan/run_gate.rs:49`
- `tomlctl/src/plan/approve.rs:208`
- `tomlctl/src/plan/emit.rs:177`

I left them alone, as the spec says. The tasks that own those handlers add the new fields to their patterns.
