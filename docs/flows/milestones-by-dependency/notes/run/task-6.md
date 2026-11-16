Status: done — plan check gains the after-edge errors, the pre-approval order warning, and a milestone-width that never targets a deferred milestone

```toml run-note
files_touched = ["tomlctl/src/plan/check.rs", "tomlctl/tests/plan_check.rs"]

[[tangential]]
kind = "question"
area = "tomlctl/src/plan/check.rs"
summary = "plan/milestone-order-unqualified fires on every pre-approval plan with two milestones that share no edge, including an empty or deferred milestone ahead of an independent one"
why = "Plans in elaboration will see this warning often; the later documentation task may want to say that a plan can quiet it with --after or accept it as intended"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/check.rs tomlctl/tests/plan_check.rs"
result = "pass (after applying rustfmt to plan_check.rs)"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "grep -qF for the three class names and milestones::upstream( in tomlctl/src/plan/check.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^plan_check::/)'"
result = "pass (55 tests)"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^plan_probes::re_probing/) | test(/^plan_milestone::/) | test(/^tasks_audit::/)'"
result = "pass (76 tests)"
```

## What changed

**tomlctl/src/plan/check.rs**

- `milestone_findings` now checks each `[[milestones]]` row's `after` list against
  `[[checkpoints]]` store order: `plan/milestone-after-unknown` (error) for an id naming
  no checkpoint, and `plan/milestone-after-forward` (error) for an id naming the
  milestone itself or one stored at or after it. Both carry the milestone's row ids.
  When the milestone has no checkpoint of its own, only a self-reference counts as
  forward.
- New `milestone_order_findings`, which runs only inside the `if plan.approvals.is_empty()`
  block. It calls `milestones::upstream(tasks)`, takes each milestone's transitive
  upstream set, and for every milestone after the first emits one
  `plan/milestone-order-unqualified` warning with empty ids, naming the earlier
  milestones outside that set and pointing at `--after <id>`. When `upstream` refuses
  (an unknown `after`, a dangling edge or a cycle), it raises nothing, because the
  after-unknown class and the task store's dag classes already report the cause.
- `milestone_width_findings`: when the computed target names a deferred milestone, the
  target moves to the next milestone after it that is not deferred and is earlier than
  the row's own. When there is none, the row is skipped.
- A shared `checkpoint_order` helper (deduped `[[checkpoints]]` ids) is used by the width
  check, the order check and the after checks. The module header lists the new warning.

**tomlctl/tests/plan_check.rs**

- Two existing tests were updated for the new warning: `milestone_width_reports_a_later_row_with_no_needs_at_info`
  now expects the order warning beside the width info, and
  `milestone_width_is_not_raised_for_a_deferred_milestone` now expects only the order warning.
- New tests:
  - the after-unknown error
  - the after-forward error, for itself and for a later milestone
  - an accepted earlier `after`
  - the order warning's detail and empty ids
  - `after`, `coupling` and transitive `after` all quieting the order warning
  - no order warning once the plan is approved
  - the width change, covering both the moved target and the skipped row

Per the constraint, I did not run the `cli::dispatch::tests` subset. The new class names
are documented by a later task.
