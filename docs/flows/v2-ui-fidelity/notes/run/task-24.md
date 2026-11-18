Status: done — elaborate.md and approve.md carry the new surfacing rules

```toml run-note
files_touched = ["claude/skills/plan-v2-engine/references/elaborate.md", "claude/skills/plan-v2-engine/references/approve.md"]

[[checks]]
command = "accept grep chain (ui-layout.md, four, silent one-way, grouped by class)"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "fail"
```

## What changed

- elaborate.md section 2: first-round budget of four in one AskUserQuestion call, follow-ups in further calls outside it, "zero survivors is the normal case" removed, a UI-milestone paragraph pointing to ui-layout.md and design/direction.md. Section 5 gained the pre-flip listing of agent-assumed one-way unsurfaced decisions. Section 6 commit stages every file under design/, pictures included.
- approve.md: refusal row for a silent one-way door; assumptions shown grouped by class.

## Check note

The dispatch test subset had 2 failures, both in finding_classes (plan finding classes read from Rust source: the_plan_scan_reads_both_construction_sites_and_no_tasks_class and documented_plan_classes_match_the_source). They concern tomlctl plan sources and docs that sibling tasks are editing, not the two files held here; not repaired.
