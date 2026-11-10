Status: done — `review triage --answer-file`, the `review settle` verb with its three outcomes, deferred-proposal ageing in the digest, and the overdue refusal in `review run finish` are in place.

## What changed

- `tomlctl/src/review/settle.rs` (new)
  - Holds `review settle <id> --accept --tier <t> --fix-file <p> | --refute --citation <c> | --defer --proposal <p>`.
  - Refusals:
    - a finding that is not `open` or `scheduled`;
    - a finding not blocked on `evidence`, which also covers one that is not blocked at all;
    - `--defer` on a finding that already carries `proposal`.
  - Every outcome first moves the finding to `open` and removes `apply_flow` and `apply_task`. Then:
    - accept clears the block, sets `fix_sketch`, `settle_tier` and `unblocked_by = "settle"`, and drops `proposal` / `proposal_at`;
    - refute records an `orchestrator` `refuted` verdict through `verdict::orchestrator_verdict`, and that verdict decides the status;
    - defer sets `proposal`, a repo-relative path that must exist inside the repo, and `proposal_at`, a UTC TOML offset datetime to whole seconds.
  - Replaying an outcome that is already recorded writes nothing and reports `unchanged: true`.
  - Also holds the ageing helpers. `deferred(store, family)` lists the open findings of that family that carry `proposal`. Each one's age is the number of runs of the family whose `started` is later than `proposal_at`. `OVERDUE_AGE` is 3, and `Deferred::overdue()` is true at that age or more.
- `tomlctl/src/review/verdict.rs`: one addition, `orchestrator_verdict()`. It builds the row and runs it through the existing `apply_one`, so the checks and status decision are unchanged. Nothing else in the file changed.
- `tomlctl/src/review/triage.rs`: new `Mode::Answer`, read from `--answer-file` and capped by the catalogue's `decision_answer` cap.
  - Refused on a finding that is not `open`, or not blocked on `decision`.
  - On landing it clears the block but keeps `blocked_options`, and sets `decision_answer` and `unblocked_by = "user"`. The status is not touched, so `reopen_reason` survives.
  - The replay check moved into `Mode::recorded`, which answers for both the new mode and the existing companion modes.
- `tomlctl/src/review/fields.rs`: new `SETTLE_TIERS`. Four new computed defect/cleanup catalogue fields, each with a projection-matrix row:
  - `decision_answer` (BLOCKED visibility, cap 800);
  - `settle_tier` (TIER visibility);
  - `proposal` (INTERNAL);
  - `proposal_at` (INTERNAL).
- `tomlctl/src/review/digest.rs`: the digest gains a `proposals` array. It lists every deferred proposal of the run's family with `id`, `kind`, `summary`, `proposal`, `proposal_at`, `age` and `overdue`.
- `tomlctl/src/review/runs.rs`: `run finish` refuses while any proposal of the run's family is overdue, naming each as `F<n> (age N)`. `--incomplete` does not waive this. When no proposal is overdue, behaviour is unchanged.
- `tomlctl/src/cli/types/review/triage.rs`:
  - new `ReviewSettleArgs` and `ReviewSettleOutcome` (a required group of one; each outcome `requires` its companions, and each companion conflicts with the other outcomes);
  - `--answer-file` in the triage mode group;
  - `answer_file` added to the `NOT_DEFER` and `NOT_CONVERT` conflict lists.
- `tomlctl/src/cli/types/review/mod.rs`: new `ReviewOp::Settle` variant.
- `tomlctl/src/review/dispatch.rs`, `tomlctl/src/review/mod.rs`: route `review settle` and declare the module.
- `tomlctl/tests/review_settle.rs` (new): accept, refute, and defer with a refused second deferral, each starting from a scheduled, linked finding; refusal of a finding blocked on decision or not blocked; `settle_digest_age_counts_later_runs_of_the_family`.
- `tomlctl/tests/review_triage.rs`: `answer_unblocks_a_decision_finding_keeping_its_options`, `answer_is_refused_on_a_finding_not_blocked_on_a_decision`.
- `tomlctl/tests/review_finish.rs`: `finish_refuses_while_a_proposal_is_overdue`, `finish_overdue_proposal_cleared_by_triage_moving_it_off_open`.

## Deviations and choices

- **Settle args are passed as a `Request`, not as the clap type.** `ReviewSettleArgs` cannot be imported as `crate::cli::ReviewSettleArgs`: the re-export list lives in `tomlctl/src/cli/mod.rs`, which this task does not hold. `dispatch.rs` therefore unpacks the flags into a `settle::Request`, following the existing `schedule::Request` idiom. The alternative was to fail the task to widen its files for a one-line re-export. Nothing a user sees differs.
- **`proposal_at` is catalogued as `Ty::Date`.** The catalogue has no datetime type, and adding one means editing exhaustive matches in `schema_out.rs`, which is outside the task's files. `Ty::Date` is the catalogue's TOML-datetime type, and the store reads a full offset datetime back through it. The field is computed, so it never appears in an input schema.
- **Accept drops `proposal` and `proposal_at`.** The constraint defines overdue as an open finding that carries `proposal`. An accepted finding is open, so if it kept `proposal` it would stay overdue after its disposition. Refute keeps them: the finding is no longer open, so it stops ageing.
- **A missing or unreadable `proposal_at` counts every run of the family.** This leans toward forcing a disposition on a damaged row rather than letting it go unnoticed.
- **`--incomplete` does not bypass the overdue refusal.** D12 says a run cannot close until an overdue proposal has a disposition, and `--incomplete` waives coverage gaps, not owed dispositions. Overruling this is a one-line change in `runs::finish`.
- **Status rules I added beyond the constraints.** Settle refuses a finding that is not `open` or `scheduled`, so it never revives a dismissed or resolved row. `--answer-file` refuses a finding that is not `open`.
- **The refute rationale is fixed text.** The design gives refute only `--citation`, so the verdict's required `rationale` is the constant "its settle proposal refuted the finding".

## Checks

- Compile: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` is clean.
- Format: I ran `rustfmt --edition 2024` on every edited leaf file and applied it to settle.rs, triage.rs and the three test files. `tests/common` is unchanged. fields.rs and mod.rs have `mod` children, so they were not formatted and are left to the gate.
- Accept "the settle module exists" (`test -f tomlctl/src/review/settle.rs`): passes.
- The build-writing accept checks belong to the gate. I ran the five affected binaries once to confirm: `review_settle`, `review_triage`, `review_finish`, `review_digest` and `review_verdict`, 68 tests, all passed.
- Lib `review::` tests, including the projection matrix: they pass. Two `cli::dispatch::tests::lint` failures (`flag_table_lint`, `command_lint`) both name `--fix-shape` on `review list`. They predate this task; the earlier task that retired that flag noted them, and the docs that still name it belong to other tasks.

## Handover

None.

## Tangential

TANGENTIAL: debt | claude/skills/tomlctl/references/review.md | The flag tables do not yet list `review settle` or `review triage --answer-file` | The flag lint checks only doc→CLI, so the gap fails no gate, and orchestrators will not learn the verbs until the reference names them
TANGENTIAL: question | tomlctl/src/review/settle.rs | `settle --refute` on a panel-routed finding is refused, because `review verdict` takes only panel and audit verdicts on that route | A panel-routed finding blocked on evidence cannot be refuted at settle; it would need an audit verdict or a rule for this case
TANGENTIAL: debt | tomlctl/src/cli/types/mod.rs | No capability is advertised for `review settle`, `--answer-file` or proposal ageing | The apply carrier's pre-flight cannot detect an installed tomlctl that lacks them
