Status: done — milestones store gate probes; `plan milestone add|update --gate-probe`, `plan probes --emit|--record --gates` and `plan/gate-probe-unknown` are in place.

## What changed

- `tomlctl/src/tasks/schema.rs`: new `GateProbe {gate, probe, verdict, at}`; `MilestoneSpec.gate_probes` with its codec. `[[milestones.gate_probes]]` is written only when the list is non-empty, so a milestone without probes keeps its bytes. The round-trip fixture in the unit tests carries one probe.
- `tomlctl/src/cli/types/plan.rs`: repeatable `--gate-probe '<gate> :: <probe>'` on `plan milestone add` and `update`; `--gates` on `plan probes`, conflicting with `--premises` and `--task`.
- `tomlctl/src/plan/milestone.rs`: splits each value at the first ` :: `. It refuses a value with no separator, an empty side, a probe identical to its gate, or a gate that is not one of the milestone's gates (on update, checked against the gates the same patch leaves). On update the given probes replace the whole list, as `--gate` does. A pair already stored keeps its recorded verdict. Audited stores get a `gate_probes` change row as compact JSON of the `{gate, probe}` pairs.
- `tomlctl/src/plan/probes.rs`: `--emit --gates [--milestone]` prints one `p <milestone>:<index> gate <digest> '<probe>'` line per stored probe through the same helper and header, and still runs nothing. `--record --gates` parses the capture with a `<milestone>:<index>` key and the existing row shape, refuses a stale digest, a short capture, extra rows and any non-gate control, and stores `holds`, `fails` or `broken` by `premise_verdict` with `at` = the capture head. The capture parser is now generic over its key parser (`parse_capture_keyed`); the accept and premise paths parse exactly as before and now also refuse a gate row (one whose milestone id is numeric would otherwise parse as a task key). The milestone id goes into the script unquoted as part of the key, so the emit refuses an id outside `[A-Za-z0-9._-]` for a milestone that holds probes.
- `tomlctl/src/plan/check.rs`: `plan/gate-probe-unknown` (error, ids = the milestone's tasks) when a stored probe's gate is no longer one of the milestone's gates.
- Tests: `milestone_gate_probe_round_trips` and `gate_probe_must_name_a_gate` in `tomlctl/tests/plan_milestone.rs`; `gate_probes_emit_and_record` in `tomlctl/tests/plan_probes.rs`.

## Deviations

- The detail does not say how `update --gate-probe` merges with what is stored. I chose to replace the whole list, which matches `--gate` on update, and to keep the verdict of an unchanged pair. The alternative was to add or replace per gate, which would need a separate removal flag the design does not name. I added no `--clear-gate-probe`; dropping probes means passing the remaining ones, and a probe left behind by a gate change is what `plan/gate-probe-unknown` reports.
- Record output for `--gates` is `{"ok":true,"recorded":[{"milestone","gate_probe","verdict"}]}`. The design leaves the shape open, so I modelled it on the premises output.

## Handover

None.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on each edited file: clean. `probes.rs` and the two test files were reformatted, and only my added lines changed.
- `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(gate_probe)'`: 3 passed.
- I also ran the neighbouring binaries and unit tests, `plan_probes`, `plan_milestone`, `plan_check`, `tasks::schema::` and `plan::milestone::`: 154 passed.
- All three accept greps: pass.
- `cargo test --lib -- cli::dispatch::tests`: 53 passed, 1 failed. `documented_plan_classes_match_the_source` reports that `plan/gate-probe-unknown`, `plan/accept-owner-invalid` and `plan/accept-cover-unknown` have no row in `claude/skills/tomlctl/references/plan.md`. That file belongs to the documentation task in this milestone, which adds those rows; the two `accept-*` classes come from the earlier accept-owner task. The parser did read the new class from source.

TANGENTIAL: none
