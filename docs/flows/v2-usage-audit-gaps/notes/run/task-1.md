Status: done — plan store schema 7 shapes added, stamped 7 only when one is present, documented in plan-store.md

```toml run-note
files_touched = [
  "tomlctl/src/plan/schema.rs",
  "tomlctl/src/plan/approve.rs",
  "tomlctl/src/plan/records.rs",
  "tomlctl/src/plan/run_gate.rs",
  "tomlctl/src/plan/run_notes.rs",
  "claude/skills/tomlctl/references/plan-store.md",
]

[[deviations]]
what = "The schema 7 stamp is raised by any written new key on a gate command (base_checked, base_failed_ids or base_whole_failed), not only by base_checked = true"
why = "Each key is written on its own when set; a store carrying base_failed_ids or base_whole_failed without base_checked would otherwise be stamped 6 and a schema 6 binary would drop the key silently. The holds_gate_accept_shapes predicate counts every key that is written."
means = "design/records.md, Plan store schema 7: stamp 7 for a gate command with base_checked = true"

[[deviations]]
what = "schema_request.rs was left unedited"
why = "Its PlanStore literals all use ..fixture() and its tests assert tiers no schema 7 shape touches, so nothing there needed a change to compile or stay correct."
means = "files list naming tomlctl/src/plan/schema_request.rs as edit"

[[deviations]]
what = "Attempt sources read fetched_at as a required datetime; claim and url-or-query are read leniently by the store"
why = "fetched_at has no sensible empty default for a Datetime field; enforcing claim and url-or-query is the note parser's job, which turns a malformed note into a problem line instead of a store that refuses to load."
means = ""

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/{schema,approve,records,run_gate,run_notes}.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "fail"

[[checks]]
command = "grep -q 'GATE_ACCEPT_SCHEMA_VERSION: i64 = 7' tomlctl/src/plan/schema.rs"
result = "pass"

[[checks]]
command = "grep -q 'base_failed_ids' tomlctl/src/plan/schema.rs && grep -q 'delta_waiver' tomlctl/src/plan/schema.rs"
result = "pass"

[[checks]]
command = "grep -q 'gate_accept' claude/skills/tomlctl/references/plan-store.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed

- `tomlctl/src/plan/schema.rs`: `GATE_ACCEPT_SCHEMA_VERSION = 7` above `VISUAL_ACCEPT_SCHEMA_VERSION`, `SCHEMA_VERSION` pointing at it, the ordering assert extended. New `GateAcceptKind` vocabulary (`accept`, `extra-round`, `base`), `GateAccept` / `AcceptedCommand` records stored as `[[gate_accept]]` after `[[visual_accept]]`, following the visual acceptance pattern (struct, from/to TOML, store vector, write_version rung). New fields: `Approval.delta_waiver`; `Handover.blocks`, `ready_check`, `note`; `Attempt.sources` (new `Source` struct); `GateRound.tree` (new `TreeEntry`); `GateCommand.base_checked`, `base_failed_ids`, `base_whole_failed`. Each reads as empty when absent and is written only when set, so existing stores keep their bytes and stamp. `PlanStore::holds_gate_accept_shapes` decides the 7 rung, tested first in `write_version`. Tests added: each shape alone round-trips through rendered text and stamps 7; a gate acceptance is written last, outranks a visual acceptance and writes its keys in record order; stores using none of the shapes (base, run records, class, request, visual) keep their stamp and write none of the new keys; removing every shape returns to the old bytes; an unknown kind is refused naming the vocabulary; a source without `fetched_at` is refused. The file was formatted with rustfmt.
- `approve.rs`, `records.rs`, `run_gate.rs` (including its test module), `run_notes.rs`: the new fields initialised empty at each existing struct literal, nothing else.
- `claude/skills/tomlctl/references/plan-store.md`: example store at schema 7 with each new shape, the stamp rule, and key tables for the approval waiver, handover blocks/ready_check/note, attempt sources, gate round tree, gate command base-rerun keys (with the base failure definitions) and the `[[gate_accept]]` record.

## Checks

The clippy run failed with five errors, none caused by this task: three name fields that the sibling task editing `tomlctl/src/cli/types/plan.rs` added to argument structs (`waive_review_delta` destructured in `approve.rs`, `after_round` in `emit.rs`, `base`/`accept`/`json` in `run_gate.rs`), and two name `MilestoneSpec` fields that the sibling editing `tomlctl/src/tasks/schema.rs` added (`milestone.rs`). The verb tasks that use those flags will consume them. No error names a type or field from this task, so the type check of the schema 7 shapes and their literals passed. Because the crate did not compile, the narrow `cli::dispatch::tests` subset for the `.md` edit was not run. The gate runs it and the `plan::schema::` nextest accept check.
