Status: done — every kind of Plan row now shows its whole record and its links in the details panel

## What changed

- `glimpse/src/view/plan_details.rs`: `content(app, row) -> Doc`. It has one builder per record kind: decision, contract, handover, milestone, run, authored file, change, departure, cost row and health row. A small `Builder` handles titles, `label value` facts, markdown sections, command sections and task lines. It adds each link once, in the order the text mentions it.
  - **Decision.** Shows every field, the rejected options and the binds. Its links are the notes its evidence names as `notes/<id>.md` (read by `cited_notes`), then the bound tasks, then the bound paths as `Source` targets.
  - **Milestone.** The goal is the checkpoint's `rationale`. Each gate is shown with its probe and verdict, then the member tasks, then every run of every gate round from `PlanIndex::gate_rounds_for`, ordered by round and then seq. Each command shows its outcome, duration, exit code, summary, failed ids and kept log. A round above 0 is labelled as a fix round. Its links are the tasks, then the logs.
  - **Authored file.** Shows kind, state, source count, headline and sources. Sources link as `Source` targets (with their line), or as `Url` when they have no path. A run note links its task. Any other note also links back, after its sources, to each decision whose evidence names it.
  - **Run.** Shows its facts, then the dispatched tasks as links.
  - **Contract.** Shows its fields and links its tasks.
  - **Handover.** Shows its fields and links its source task.
  - **Change and departure.** Each links the task it names.
  - **Cost row.** Shows totals and each agent with its type, time and tokens. It has no links.
  - **Tests.** 11 tests, named `plan_details_*`, run over the fixture with a listing. They mutate D1's evidence in-test to cite `notes/keys.md`. They cover a milestone with a failed gate round, a note linking back to its decision, every kind's text and links, the Plan arm through `details::links`, and the empty cursor.
- `glimpse/src/view/details.rs`: added only the `Surface::Plan` arm in `shown`, plus `plan_details` in the `view` import. The border is titled by the row id. With no cursor row the panel reads "No item selected.", the same as the item surfaces.

## Deviations

These are judgement calls, not departures from a `suggested` means (the task has none).

- **Health rows link their subject.** That is the task for an accept check or premise, and the milestone for a gate probe. The design's Links table has no Health row. The alternative was no links. I chose the link because the row's own `Enter` target is the same task, and a link adds nothing a user would not expect.
- **Small local helpers in `plan_details.rs`.** `duration`, `count` and `kind_name` duplicate private helpers in `plan_state.rs` and `details.rs`. The constraints allow details.rs only the Plan arm, and `plan_state.rs` is not in this task's files.
- **Run reason label.** A run's `reason` is shown under the label "Reason", not "abandon reason", because the store field is generic.

## Checks

- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets` is clean for these files. The two remaining warnings are dead-code warnings in `src/model/plan.rs`, which is not mine.
- `rustfmt --edition 2024 --check` passes on both edited files.
- Accept check "module exists and is called" (grep): passes.
- Accept check "carries its own tests" (grep): passes.
- `cargo test ... plan_details` is the gate's to run, so I did not run it. Its assertions were checked by hand against `tests/fixtures/snapshot.json`.

## Handover

None.

TANGENTIAL: debt | glimpse/src/view | duration/token-count formatting is now duplicated in details.rs, plan_state.rs and plan_details.rs | one shared formatter would keep the panels' time and token text from drifting apart
