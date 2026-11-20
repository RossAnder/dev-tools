Status: done — plan check gains baseline-gate-unproved, handover-blocks-unknown, handover-blocks-done and ui-capture-missing, each with positive and negative tests

```toml run-note
files_touched = ["tomlctl/src/plan/check.rs", "tomlctl/tests/plan_check.rs"]

[[deviations]]
what = "A failed baseline proof counts as accepted when a decision with provenance user-stated or user-confirmed, not vetoed, has decision text containing the gate's exact command."
why = "The spec and design say 'a decision records the user accepting it' but no field or convention marks such a decision. Alternatives weighed: a dedicated class or bind form (needs a schema change outside this task's files), or a gate_accept record (that record is keyed to a gate round, not to Prove). Text containment needs no schema change and is easy to unwind."
means = "detail: plan/baseline-gate-unproved fires when the latest proof is fail and no decision records the user accepting it"

[[deviations]]
what = "plan/baseline-gate-unproved and plan/ui-capture-missing skip a deferred milestone."
why = "A deferred milestone gets its proofs and capture states at elaboration, so a warning before then would fire on every deferred UI or baseline milestone at approval. This follows ui_scope_findings and ui-accept-structural, which skip deferred milestones too."
means = "detail: warn on a milestone whose ui is true and whose ui_states is empty"

[[deviations]]
what = "plan/handover-blocks-unknown and plan/handover-blocks-done are raised only for a handover that is not done."
why = "handover_findings already returns early for a done handover, because it has nothing left to run and must not block plan approve --amend; a done handover's blocks hold nothing back."
means = "detail: plan/handover-blocks-unknown (error): a handover's blocks names no task"

[[deviations]]
what = "plan/ui-capture-missing follows the retargeted rule (ui = true and no ui_states, config or not) and no longer reads the capture config or look decisions."
why = "The detail's last part retargets the class this way, and design/records.md#task-store-milestone-fields says the same."
means = "detail: original root.join(ui_capture::CONFIG) existence trigger"

[[tangential]]
kind = "question"
area = "claude/skills/plan-v2-engine/references/write-and-prove.md"
summary = "When Prove records a user accepting a failed baseline proof, the decision's text must contain the gate command verbatim, from a user-stated or user-confirmed decision, for plan check to stop warning."
why = "The prose tasks that teach Prove (and plan.md's class entry) should state this rule, or the warning keeps firing after the user accepts."

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "fail"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --test it"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/plan_check.rs"
result = "pass"

[[checks]]
command = "grep -q 'plan/baseline-gate-unproved' tomlctl/src/plan/check.rs && grep -q 'plan/handover-blocks-unknown' tomlctl/src/plan/check.rs && grep -q 'plan/handover-blocks-done' tomlctl/src/plan/check.rs && grep -q 'plan/ui-capture-missing' tomlctl/src/plan/check.rs"
result = "pass"
```

## What changed

`tomlctl/src/plan/check.rs`:
- Module header lists the three new warnings and names `plan/handover-blocks-unknown` as an error.
- `handover_findings` now also reports, for an undone handover, `blocks` ids no row carries (`plan/handover-blocks-unknown`, error) and `blocks` ids whose row is already done (`plan/handover-blocks-done`, warning).
- New `baseline_findings`: for each baseline gate of a milestone that is not deferred, it warns `plan/baseline-gate-unproved` when there is no proof, when the proof is `fail` and no user decision accepts it (`baseline_failure_accepted`), when a `null-control` proof's `task` is missing, is no row of the milestone, or has no accept entry (`null_control_gap`), or when the verdict is unknown. The finding names the milestone's rows.
- New `ui_capture_findings`: warns `plan/ui-capture-missing` on a milestone that is not deferred, has `ui = true` and has no `ui_states`.

`tomlctl/tests/plan_check.rs`: six tests. Baseline: no proof and a fail proof warn, a pass proof is clean, a deferred milestone is clean. A user-confirmed decision naming the gate clears a fail, and an agent-assumed decision or one naming another gate does not. A null-control proof is clean on task 1 and warns on an absent task, on a task of another milestone, and on a task with no accept. Handovers: blocks on a known task are clean, an unknown id is an error naming only that id, a done row blocked by an undone handover warns, and a done handover is clean. UI: `ui = true` with no states warns with or without a capture config, and `ui = false` or `ui = true` with states is clean.

## Checks

The full `--all-targets` clippy fails only in the lib-test target, at `tomlctl/src/plan/records.rs` (a call with the wrong argument count). That is another worker's in-flight edit, not this task's file. The lib target and the `it` integration binary, which holds `plan_check.rs`, lint clean. `check.rs` has `mod` children, so I ran rustfmt in check mode only and read its diffs for `check.rs`, and there are none now. The `plan_check::` nextest accept is the gate's to run, so I did not run it. The finding-class parity test stays red until task 14 adds the four classes to `plan.md`, as the spec expects.
