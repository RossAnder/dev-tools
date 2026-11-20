Status: done — gate-list --after-round narrows after a whitespace-only round; launch-args marks building tasks and withholds handover-blocked ones

```toml run-note
files_touched = ["tomlctl/src/plan/emit.rs", "tomlctl/tests/plan_emit.rs", "claude/skills/tomlctl/references/plan-engine.md"]

[[deviations]]
what = "A narrowed `commands` keeps every entry of the current full list that round n did not record as pass or flaky, rather than only round n's fail/timeout commands; `carried` holds the full-list entries round n passed. A round-n command no longer in the list is dropped."
why = "A command the list holds now but round n never ran (an accept check a fix row added) was never run at all, so carrying it would skip it; taking entries from the full list also keeps each one's `source`. On the plain case (same list, every command ran) the result is exactly the spec's."
means = "design/records.md#emit-outputs: commands as round n's commands whose outcome was fail or timeout"

[[deviations]]
what = "`--after-round` on a flow with no plan.toml, or naming a round with no recorded run, prints the full list with narrowed = false instead of refusing."
why = "The spec's 'any other case' rule; refusing would turn a missing record into a blocked gate."
means = "design/records.md#emit-outputs"

[[deviations]]
what = "When a task is blocked by several undone handovers, the reason names the first in store order; a premise withholding outranks a handover one."
why = "The spec names one reason per drop; the existing build keeps the first reason that dropped a row."
means = "detail: withheld with reason handover:<id>"

[[tangential]]
kind = "other"
area = "tomlctl/src/plan/check.rs"
summary = "The lib test target did not compile during this run: check.rs calls missing baseline_findings/ui_capture_findings, and records.rs:937 calls a 4-argument fn with 3."
why = "In-flight edits of other tasks; it blocked the cli::dispatch::tests run after the plan-engine.md edit."

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/emit.rs tomlctl/tests/plan_emit.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --test it"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "fail"

[[checks]]
command = "grep -q 'after_round' tomlctl/src/plan/emit.rs && grep -q '\"builds\"' tomlctl/src/plan/emit.rs && grep -q 'handover:' tomlctl/src/plan/emit.rs"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed

- `tomlctl/src/plan/emit.rs`
  - `gate-list` reads `after_round` (the flag was already declared in `cli/types/plan.rs`). With it, `whitespace_round` finds round n's latest run by `seq`, requires a non-empty `tree`, and checks every normalised path in the milestone rows' `files`. Each path must have a tree entry and be either unchanged (`raw` equal) or whitespace-only (`stripped` equal) and not whitespace-significant (`.py .yaml .yml .md .sh .toml .json`, or a `Makefile`, both matched case-insensitively). The digests come from `run_gate::whitespace_digest`, so they are the same ones `plan run gate` records. When the round qualifies, the passed and flaky entries move to `carried` and `narrowed = true`. Otherwise `narrowed = false` and the list stays full. Without the flag the output is byte-for-byte unchanged, and `held`, `skipped` and `visual` are untouched either way.
  - `launch-args` loads `plan.toml` when it exists (`optional_plan`). `handover_holds` maps each task named in the `blocks` of an undone handover to that handover's id. `Launch::build` withholds such a pending task as `handover:<id>` and cascades its dependents as `needs-withheld:<id>`, the same way a withheld premise does. Each dispatched task whose spec holds an accept entry with owner `worker` and `writes_build = true` gets `builds: true`. The key is absent otherwise, so `args` and `args_sha256` stay unchanged for stores without such checks.
- `tomlctl/tests/plan_emit.rs`: four new tests:
  - a whitespace-only `.rs` change narrows the list;
  - a whitespace change to a `.md` file, a token change, a path with no tree entry and a round with no tree each keep the full list;
  - a building worker task is marked, and the hash still matches;
  - a done handover holds nothing, an undone one withholds its task and cascades to the dependent.
- `claude/skills/tomlctl/references/plan-engine.md`: documents `--after-round`, with its flag row, the `carried` and `narrowed` keys and the rule with its accepted limits. It also documents the `builds` key and the `handover:<id>` drop reason.

## Checks

The plan_emit nextest accept check belongs to the gate and was not run here. `cargo clippy --all-targets` failed only in the lib test target, on `tomlctl/src/plan/records.rs:937`, which is another task's in-flight edit. The lib itself and the `it` test binary, which includes these tests, compile with clippy clean in my files. `cli::dispatch::tests` could not build, because the lib test target fails on `plan/check.rs` (a sibling) and `plan/records.rs`. That run is left to the gate.

The row was marked `in-progress` late, after the edits rather than before them. No other worker held it.
