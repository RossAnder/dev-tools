Status: done — plan approve refuses a silent agent-assumed one-way door, plan stage holds it at prove, and the gate groups assumptions by class

```toml run-note
files_touched = ["tomlctl/src/plan/store.rs", "tomlctl/src/plan/approve.rs", "tomlctl/src/plan/stage.rs", "tomlctl/tests/plan_approve.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/store.rs tomlctl/src/plan/approve.rs tomlctl/src/plan/stage.rs tomlctl/tests/plan_approve.rs"
result = "pass"

[[checks]]
command = "grep -q 'silent_one_way' tomlctl/src/plan/store.rs && grep -q 'silent_one_way' tomlctl/src/plan/approve.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/silent_one_way/)'"
result = "not-run"
```

## What changed

- `tomlctl/src/plan/store.rs`: new `silent_one_way_decisions`, a sibling of
  `unanswered_decisions`. It names each decision that is `agent-assumed`, `door = one-way`
  and not `surfaced`, and tells the user to surface each one with `plan decide`. A
  user-stated or user-confirmed decision is never named. A surfaced decision that is still
  agent-assumed is also never named, because `unanswered_decisions` already refuses it.
- `tomlctl/src/plan/approve.rs`: `approve` refuses on the new predicate right after the
  unanswered-decision refusal. In `gate_report`, `assumptions` is still a flat list, now
  sorted by a key of (class rank, widest binds first). The class rank is the
  `DecisionClass` variant's declaration order, which matches the vocabulary order
  (structure, look, ux, cost, vocabulary, architecture, compromise), and unclassed
  decisions come last. The sort is stable, so store order still breaks ties. `assumption()`
  emits `class`, which is null when unset.
- `tomlctl/src/plan/stage.rs`: a new `undecided()` helper chains both predicates. The
  prove-stage blocker and `amend_report` both use it, so stage never advises an `--amend`
  that approve would refuse, and `next` resolves to `plan decide` for either case.
  `view.rs` is unchanged, as the detail asks.
- `tomlctl/tests/plan_approve.rs`: three `silent_one_way_*` tests. One checks that approve
  refuses a silent one-way door and that stage holds it at prove with `plan decide`. One
  checks that a user-stated or user-confirmed one-way door passes. One checks that a
  surfaced agent-assumed one-way door falls to the unanswered rule and not the new one.
  Also one gate test for the class grouping and the null `class`.

The `silent_one_way` nextest accept check is the gate's to run, so I did not run it here,
and the new tests have not been run yet. Clippy over all targets is clean.

The existing fixtures that approve or stage a one-way agent-assumed decision all set
`surfaced = true` (`plan_lifecycle.rs`, `review_apply.rs`, `review_plan_records.rs`), and
`GATE_PLAN`'s one-way D2 is only ever used by `plan gate`. So none of them should start
being refused. The existing gate-order test's `["D2", "D1"]` still holds because both
decisions are unclassed.
