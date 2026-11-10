Status: done — `review schedule --settle-checkpoint` mints one settle task per evidence-blocked finding with no proposal, refuses every other blocked finding with its reason, and carries `decision_answer` and `fix_sketch` onto fix tasks

## What changed

- `tomlctl/src/cli/types/review/export.rs`: `ReviewScheduleArgs` gains `--settle-checkpoint <ID>` (`Option<String>`).
- `tomlctl/src/review/dispatch.rs`: in the `ReviewOp::Schedule` arm only, passes `settle_checkpoint: args.settle_checkpoint.as_deref()` into `schedule::Request`. The other uncommitted lines in this file (the `ReviewOp::Settle` arm and the `settle` import) came from an earlier task and are untouched.
- `tomlctl/src/review/schedule.rs`:
  - `Request` gains `settle_checkpoint: Option<&str>`.
  - `selection` now returns the settle candidates along with the `Selection`. Blocked findings are found through `fields::blocked_on`, the C3 helper, so the `--all` loop no longer compares `fix_shape` with `needs-plan`. In explicit-id mode they come from the per-family refusals; under `--all`, from a pass over the store. Each one goes to `settle_refusal`:
    - evidence with a `proposal`: refused ("its proposal ... is already made, and a finding has one settle round");
    - evidence without the flag: refused, naming `--settle-checkpoint`;
    - measurement or alternatives: refused, "it waits for trials";
    - decision or design: refused with `select::refusal`'s text, which keeps the legacy needs-plan suffix;
    - evidence with no proposal and the flag set: it becomes a settle candidate.
  - The "every finding was refused" error also requires that there are no settle candidates.
  - `Compose::plan` adds the settle rows after the fix rows, so the predicted ids stay contiguous. A finding that a live (uncommitted, not deferred) row already `settles` is linked instead of getting a second row. `live_task` takes a `settle` flag that picks `settles` or `fixes` from `[[finding_links]]`.
  - `Compose::settle_row`:
    - `files` is `docs/flows/<flow>/notes/settle/<id>.md`, with the file note `(new)`;
    - it joins the settle checkpoint and runs on executor `deep`;
    - `outcome` is that the note opens `Proposal:` or `Refuted:` and answers `blocked_missing`;
    - the accept check is `head -n 1 <note> | grep -Eq '^(Proposal|Refuted):'`, `forward`, `writes_build = false`;
    - it carries `settles`, `review_store` and `reason` the same way fix rows do.
  - `Compose::row`: each finding's `decision_answer` adds a `Decision: <answer>` constraint, and a non-empty `fix_sketch` appends `Fix sketch: <sketch>` to that finding's block in `detail`.
  - `Placed` and `NewRow` carry a `settle` flag. The output's `settle` list is now filled, and `mark_scheduled` marks settle findings `scheduled` with `apply_task` set to the settle row's ref.
- `tomlctl/tests/review_schedule.rs`: six new tests:
  - `an_evidence_blocked_finding_becomes_one_settle_task`: files, `(new)`, the accept check, `settles` link, deep executor, and the finding marked `scheduled`;
  - `an_evidence_blocked_finding_is_refused_without_a_settle_checkpoint`, for both explicit ids and `--all`;
  - `a_finding_with_a_proposal_gets_no_second_settle_task`;
  - `a_settle_rerun_links_the_live_settle_task`;
  - `a_settle_flag_leaves_a_measurement_block_waiting_for_trials`;
  - `a_settled_decision_answer_binds_the_fix_task`, which also checks the fix sketch.

## Deviations

- On a folded fix task (more than one finding), the constraint reads `Decision on <id>: <answer>`. A single-finding task gets the design's literal `Decision: <answer>`. I chose this because a bare `Decision:` on a task carrying several findings does not say which finding the answer binds. The alternative was the literal text on every task, which is simpler but ambiguous when findings are folded.
- The design leaves several settle-row details open. I chose these:
  - effort `M`, to match its `deep` executor;
  - title `Settle <id>: <summary>`;
  - action and detail built from the finding's description, its `blocked_missing` and what the note's first line must say;
  - no premises.
  
  If `blocked_missing` is empty, the finding's summary stands in for it.

## Checks

- Compile check: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` is clean.
- Format: `rustfmt --edition 2024` was applied to `schedule.rs`, `export.rs` and `tests/review_schedule.rs`. `rustfmt --edition 2024 --check` on `dispatch.rs` is clean.
- Accept `grep -q 'settle_checkpoint' tomlctl/src/review/schedule.rs` (writes build: no): passes.
- Contract C3's grep over select/export/digest/query/schedule: passes.
- Accept `cargo nextest ... 'binary(review_schedule) & test(settle)'` is marked writes build: yes, so it is the gate's to run. I ran the single `review_schedule` binary anyway to check the new tests: 16 of 16 pass.
- `cli::dispatch::tests` (a narrow lib subset): `flag_table_lint` and `command_lint` fail. Both name only `--fix-shape` on `tomlctl review list`, in `claude/skills/tomlctl/references/review.md` and `claude/skills/review-v2-engine/SKILL.md`. Those files are outside this task and are another task's in-flight change. The new `--settle-checkpoint` flag does not appear in either failure.

## Handover

None. Task 23 covers the documentation of `--settle-checkpoint` in the review reference.

TANGENTIAL: bug | claude/skills/tomlctl/references/review.md | `flag_table_lint` and `command_lint` fail because the docs still name `review list --fix-shape`, which the CLI no longer defines | the pre-commit `cli::dispatch::tests` step will block the milestone commit until the docs follow the flag change
