Status: done — milestone C's five feature tokens, the four output-options cases and the reference text are in place, and the `plan` output-options group now passes because task 41 switched `plan/emit.rs` to the read printer

## What changed

All edits came from the earlier pass of this task. This run confirmed them and changed no file other than this note. It found the edits in place, and `tomlctl/src/plan/emit.rs` now uses `print_json_line`, which task 41 landed. That removed the only cause of the earlier `failed`.

- `tomlctl/src/cli/types/mod.rs`: `FEATURES` gains `accept_run_by`, `reproduce_status_milestone`, `gate_probes`, `plan_emit` and `agents_audit`. `plan_view_writes_build` stays.
- `tomlctl/tests/capabilities.rs`: the expected list gains the same five tokens.
- `tomlctl/tests/output_options.rs`:
  - three `plan emit` read cases: `launch-args`, `gate-list` and `reproduce-units`, each with `--slug plan-fixture --milestone A`;
  - an `agents audit --transcripts transcripts` read case;
  - the transcript fixture that case reads.
- `tomlctl/README.md`: the five tokens in the capabilities sample and the feature table, plus usage lines for `plan emit`, `agents audit` and `review reproduce-status --flow --milestone`.
- `claude/skills/tomlctl/SKILL.md`: a feature-table row, Quick Reference rows for `plan emit` and `agents audit`, and an `agents audit` row in the Rows-and-header table.
- `references/plan.md`:
  - the `plan emit` section;
  - `plan run start --tasks`;
  - `--gate-probe` on milestone add and update;
  - the worker view's `Run by:` line;
  - the `plan/accept-owner-invalid`, `plan/accept-cover-unknown` and `plan/gate-probe-unknown` class rows.
- `references/plan-probes.md`: the `--gates` mode, the effective-owner rule for which accept scripts are probed, and a Gate probes section.
- `references/plan-store.md`: the run row's `tasks`.
- `references/tasks-store.md`: an Accept owner block covering `run_by`, `covered_by`, the effective owner, refusals and check classes. It also adds `gate_probes` on `[[milestones]]`.
- `references/tasks-write.md`: `run_by` and `covered_by` under `tasks add-many`.
- `references/review-apply.md`: the `--flow`/`--milestone` selector of `reproduce-status`, and the gate-owned milestone accept entry that `schedule` now mints.
- `references/agents.md`: the `agents audit` section.

## Deviations

- `plan.md` gives the `plan emit` leaf flags as prose bullets under one heading, not as one flag table per leaf. `flag_table_lint` attributes a table to its heading's verb, and a heading per leaf would push `plan.md` past the 600-line reference ceiling (it has 593 lines).
- `plan-probes.md` gained the effective-owner sentence and a Gate probes section. Without them, its "only `writes_build = false` rows are probed" text would contradict the `run_by` skip.

## Handover

None. The milestone's deploy step covers `cargo install --path tomlctl`, after which the installed binary advertises the five tokens.

## Checks

- `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(every_case_honours_the_output_options::plan) | binary(capabilities) | test(every_leaf_command_has_a_case) | test(every_case_honours_the_output_options::core)'`: 73/73 pass. That is the constraint's confirmation run, widened to the capabilities binary and the `core` group. The same run built the lib and test binaries, which served as the compile check.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 54/54 pass.
- `rustfmt --edition 2024 --check` on `tests/output_options.rs` and `tests/capabilities.rs`: clean. `cli/types/mod.rs` has `mod` children, so it was left to the gate. The edit there adds only string literals.
- Accept checks, all `Writes build: no`: the five-token grep, the three emit-verb greps on `plan.md`, the `run_by`/`covered_by` grep and the output-options grep. All pass.

TANGENTIAL: debt | claude/skills/tomlctl/references/plan.md | 593 lines against the 600-line reference ceiling | the next plan verb or class row will need a split, for example moving `plan emit` or `plan stage` into its own reference
