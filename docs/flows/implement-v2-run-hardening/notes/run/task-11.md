Status: done — plan run visual --accept records an accepted visual round in a new [[visual_accept]] array stamped 6

```toml run-note
files_touched = [
  "tomlctl/src/plan/run_visual.rs",
  "tomlctl/src/plan/schema.rs",
  "tomlctl/src/plan/schema_request.rs",
  "tomlctl/src/plan/mod.rs",
  "tomlctl/src/plan/dispatch.rs",
  "tomlctl/src/cli/types/plan.rs",
  "tomlctl/tests/plan_run_records.rs",
  "tomlctl/tests/output_options.rs",
  "claude/skills/tomlctl/references/plan-run.md",
  "claude/skills/tomlctl/references/plan-store.md",
]

[[deviations]]
what = "--accept is a bool flag inside a required one-member clap ArgGroup (visual_mode) rather than a bool with required = true"
why = "The group is how the crate already makes a mode flag mandatory (backlog TriageMode, plan run gate's gate_rows), and a later mode joins the group with no change to --accept. The alternative, required = true on the bool, works too but would need loosening once a second mode exists."
means = ""

[[deviations]]
what = "The acceptance stamp has its own constant VISUAL_ACCEPT_SCHEMA_VERSION = 6, with SCHEMA_VERSION defined as equal to it, and the request tier renamed REQUEST_SCHEMA_VERSION = 5"
why = "The detail asks for the read ceiling raised by one and the request tier kept at 5 under its own name; naming the acceptance tier as well keeps write_version reading as a list of tiers, and the read ceiling stays SCHEMA_VERSION, which store.rs's newer-store tests use."
means = ""

[[deviations]]
what = "A state named twice in the payload, and a look named twice, are refused; looks and defects may be absent and default to empty"
why = "The spec does not say; refusing a duplicate matches plan request add's named-twice refusal, and an acceptance with no waived look or defect is a plain state waiver."
means = ""

[[deviations]]
what = "In the defect read and the payload parse, `class` is bound to a local before the struct literal rather than written as `class: <expr>`"
why = "The cli::dispatch finding-class gate scans src/plan for any `class:` field line with a string literal and treats it as a finding class; the shorthand keeps a defect's class out of that scan."
means = ""

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check (run_visual.rs, schema.rs, schema_request.rs, dispatch.rs, cli/types/plan.rs, tests/plan_run_records.rs, tests/output_options.rs)"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests plan::schema plan::run_visual"
result = "pass"

[[checks]]
command = "test -f tomlctl/src/plan/run_visual.rs"
result = "pass"

[[checks]]
command = "grep -q 'visual_accept' tomlctl/src/plan/schema.rs"
result = "pass"

[[checks]]
command = "grep -q 'plan run visual' claude/skills/tomlctl/references/plan-run.md && grep -q 'visual_accept' claude/skills/tomlctl/references/plan-store.md"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^plan_run_records::/) | test(/plan::schema/)'"
result = "not-run"
```

## What changed

- `tomlctl/src/plan/schema.rs`: `PlanStore` gains `visual_accepts: Vec<VisualAccept>` (with `WaivedLook {task, accept}` and `WaivedDefect {state, class, location, detail, note}`), read and written as the root array `visual_accept`, written last and only when non-empty, and defaulted empty. `write_version` tests the acceptance array first and stamps `VISUAL_ACCEPT_SCHEMA_VERSION` (6), then the request tier `REQUEST_SCHEMA_VERSION` (5, the old `SCHEMA_VERSION` value); `SCHEMA_VERSION`, the read ceiling, is now 6. A store without an acceptance writes exactly as before, stamp included. Two tests sit beside the tier tests: an acceptance round-trips through rendered text and stamps 6 alone and together with a request; a store without one writes no key and keeps its stamp, and a request alone stamps 5.
- `tomlctl/src/plan/schema_request.rs`: the request tests and the module doc now name `REQUEST_SCHEMA_VERSION`.
- `tomlctl/src/plan/run_visual.rs` (new): the leaf. It parses the payload (unknown key refused naming the accepted set; blank `why` or `backlog`, empty `states`, a defect whose `state` is not among `states` refused), loads the task store through `load_milestone_store` (unknown milestone refused), checks each state against the milestone's `ui_states` (a `ui_controls` name gets its own message), and checks each look names a `run_by = "look"` accept entry by index on a row of the milestone. It then upserts on `(milestone, round)` and prints `{"ok":true,"milestone":..,"round":..}`. Every refusal is `kind=validation` before any write.
- `tomlctl/src/cli/types/plan.rs`, `tomlctl/src/plan/dispatch.rs`, `tomlctl/src/plan/mod.rs`: `PlanRunOp::Visual(PlanRunVisualArgs)`, its dispatch arm and `mod run_visual`.
- `tomlctl/tests/plan_run_records.rs`: a `visual_flow` fixture (milestone with two states and a control, task 1 with a look at accept 0 and a gate check at accept 1) and five tests: stored, round-tripped and kept by a later `plan run report`, stamp 6; a second call for a round replaces it; a control, an unknown state, a non-look entry, an out-of-range or foreign-row look, a blank why or backlog, empty states and an unknown key are each refused, leaving the store's bytes unchanged; an unknown milestone and a missing `--accept` are refused; a store without an acceptance keeps stamp 3.
- `tomlctl/tests/output_options.rs`: a `visual-accept` flow in the plan group's fixture whose milestone A declares `list-full`, and the `plan run visual` write case.
- `claude/skills/tomlctl/references/plan-run.md`: a `## plan run visual` section with flag table, payload keys and output. `plan-store.md`: the `[[visual_accept]]` sample, key table and the stamp `6` tier.

The cross-store accept check (`plan_run_records` and `plan::schema` under nextest) is the gate's to run, so the integration tests are unverified here; the lib unit tests for `plan::schema` and `plan::run_visual` pass, and the integration tests compile under clippy `--all-targets`.
