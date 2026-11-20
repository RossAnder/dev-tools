Status: done — plan run gate --accept records accept, extra-round and base gate acceptances checked against the round's latest run

```toml run-note
files_touched = [
  "tomlctl/src/plan/run_gate.rs",
  "tomlctl/tests/plan_run_records.rs",
  "claude/skills/tomlctl/references/plan-run.md",
]

[[deviations]]
what = "The payload parsing helpers (key check, kind, why, backlog, commands) are written in run_gate.rs rather than shared with run_visual.rs"
why = "run_visual.rs's helpers are private and the file is outside this task; the shapes differ enough (a vocabulary kind, a conditional backlog, a commands array) that sharing would mean editing that file. Alternatives weighed: request run_visual.rs and lift its helpers to a shared module (less duplication, a cross-file refactor beyond the task), or duplicate the one small key-check helper (chosen)."
means = "Mirror plan run visual --accept (run_visual.rs) for payload parsing, the upsert and the output envelope"

[[deviations]]
what = "The output envelope adds `kind` beside ok, milestone and round"
why = "A base record is keyed on kind as well, so the envelope names which record the call wrote."
means = "Mirror plan run visual --accept for the output envelope"

[[deviations]]
what = "`commands` must hold at least one entry for every kind, and a non-whole base entry must name at least one failed id"
why = "records.md says a record is made against the round's failing commands; an empty list, or an id-less base entry for a command that did not fail whole at base, would waive nothing or read as the whole command. Both are easy to relax."
means = ""

[[checks]]
command = "grep -q 'extra-round' tomlctl/tests/plan_run_records.rs"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/run_gate.rs tomlctl/tests/plan_run_records.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "fail"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed

`tomlctl/src/plan/run_gate.rs`
- `--accept` now reads `--json` (refused when absent) and goes to `record_accept`, replacing the earlier placeholder refusal.
- `parse_accept` checks the payload's shape before any store is read: keys `kind`, `why`, `backlog`, `commands` only; `kind` from the `GateAcceptKind` vocabulary; `why` non-blank; `backlog` required for `accept` and `base` and empty or absent for `extra-round`; at least one command, each named once, `failed_ids` an array of strings. Each refusal is `kind=validation` and names the field or entry.
- `check_accept` runs inside the store mutation against the round's latest run: refused when that run holds a `timeout` command; each entry must name a `fail` command of the run with `failed_ids` a subset of its ids; for `base`, the command must be a base or partial base failure, its ids a subset of `base_failed_ids`, and it may be named alone only when `base_whole_failed`; an `accept` must name every failing command.
- The upsert replaces an `accept` or `extra-round` of the same (milestone, round) with either kind, and a `base` record only by another `base` record of that round. Prints `{"ok":true,"milestone":...,"round":...,"kind":...}`.

`tomlctl/tests/plan_run_records.rs`: eight tests over `base_flow` with a red round 1 — an accept naming every failing command recorded (schema version 7, every key); one omitting a failing command refused; an extra-round needing no backlog and refusing one; a table of bad payloads (blank or empty backlog on accept, an id not in the run, a command that passed, an unknown command, an unknown kind, empty commands, missing why, an unknown key); an unrecorded round and a timed-out run refused; a second record replacing the first with a base record beside it; a base record naming a non-base id, a command that passed at base, or a partial base command alone refused; a whole base failure named alone recorded.

`claude/skills/tomlctl/references/plan-run.md`: the intro mentions `[[gate_accept]]`, the synopsis gains the `--accept` form, the flag table gains `--accept` and `--json`, and a Gate acceptances section gives the keying, payload table, checks, refusals and output.

## Checks

The `extra-round` grep passes. Both edited Rust files are rustfmt-clean. `cargo clippy` fails only on files other tasks hold: `tomlctl/src/plan/approve.rs` (an unmentioned `waive_review_delta` field) and `tomlctl/src/plan/records.rs` (a test calling a function with three arguments where it takes four). It reports nothing in this task's files, but because those errors stop the build, borrowck and the lints never ran on this code. The gate's clippy is the first full check of it. The `plan_run_records` nextest check belongs to the gate. The dispatch-tests subset, which CLAUDE.md asks for after a `claude/**/*.md` edit, could not build for the same reason and was not run.
