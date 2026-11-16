Status: done — drain Next rule, lane note, flow-store note, report line, parked-rows scope and settle pairing updated

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/drain.md", "claude/skills/implement-v2-engine/references/apply-rows.md"]

[[checks]]
command = "the four accept checks"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

drain.md (586 lines): Next continues to run-milestone step 2 when committed and `--milestone` not given; lane check sentence on deferred milestones; flow-store commit sentence on a concurrent elaboration; report last line names the schedule's pick and `/plan-v2 <slug> --elaborate <id>`; parked-rows section runs for each milestone the schedule reports `parked`.

apply-rows.md: settle drain step 5 names `apply-<n>` by id pair, drops the checkpoints listing and store-order wording.
