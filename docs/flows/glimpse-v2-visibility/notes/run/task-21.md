Status: done — the Plan surface's rows, sections and `PlanState` are built in plan_state.rs, with unit tests over the fixture

## What changed

- `glimpse/src/plan_state.rs`:
  - `Section` lists the eleven sections in drawn order. Each has a `label`, which is both the heading and the prefix of every row id in it.
  - `Record` says which record a row stands for, for the details builders: an authored path, a decision, contract, handover or milestone id, a run index, a change index, an (attempt, deviation) pair, a whole `rollup::Row`, or a `Health`.
  - `PlanRow { id, section, cells, target, record, state }` implements `Tracked`, with `state` as its status.
  - `PlanState { tracking: Tracking<PlanRow>, has_plan }` has:
    - `Deref` and `DerefMut` to `Tracking`, as `InboxState` has;
    - `rebuild(snapshot, authored, cost, viewing, now)`, which keeps the cursor by id and moves a lost cursor to the row at its old visible position;
    - `visible()`, which lists every section heading, empty ones included, and nothing when there is no plan store;
    - `next_section` / `prev_section`, which skip empty sections and stay put at either end;
    - `count()`, the Health rows plus the open handovers;
    - `empty_text()`, which returns `NOT_V2` when there is no plan store;
    - `impl CursorList`.
  - `rows(snapshot, authored, cost)` builds the rows. It reads nothing but its arguments, and returns none when `snapshot.plan` is `None`.
  - There are 11 unit tests: every section's ids, each section's cells, targets and states, the health rule, changes before the approval left out, an empty section, no plan store, the count, the cursor kept across a rebuild, section jumps at both ends, and the formatters.
- `glimpse/src/surface.rs`: visibility changes only. `Tracking::{refresh, step_cursor, page_through, position_in, select_row}` and the free functions `reposition` and `item_ids` are now `pub(crate)`, so `PlanState` can share the Inbox's cursor logic instead of copying it. No behaviour changed.

## Choices and deviations

- **My own health rule.** The Health section has its own `health()` instead of `PlanIndex::health()`.
  - `PlanIndex` drops an entry that is not probeable even when it has a recorded bad verdict. The design's rule ("unprobed while probeable, or whose verdict is not `healthy` or `holds`") keeps it, and the detail says to follow the rule exactly.
  - A verdict counts as good when it is `healthy` or `holds`, for accept entries and premises alike. The other option was a good word per kind, as `PlanIndex` does it. I took the design's wording literally.
- **Rows that target nothing.** Cost rows and gate-probe Health rows have no target, as the detail says ("targets the task it names, or nothing").
  - The alternative was to target `Milestone` from a milestone cost row or a gate-probe health row. That is easy to add later if wanted.
- **Builder arguments.** The builder takes `&[AuthoredFile]` and `&[rollup::Row]` instead of `Authored` or a `PlanIndex`. It computes gate-round outcomes straight from `plan.gate_rounds`, so the caller (the wiring task) passes `authored.files` and `rollup::rollup(..)`.
- **First cursor.** The first build puts the cursor on the first row, as `InboxState::apply_inputs` does through `reposition`.
- **Notes order.** The order is note, challenge, cold read, backlog, settle, then any kind this build does not know (labelled `other`), then run notes ordered by task id. The listing's order is kept within each kind.
- **Row ids.**
  - Brief and Notes: `brief:<path>` and `notes:<path>`.
  - Decisions, contracts, handovers and milestones use their record ids.
  - Timeline: `timeline:<run index>`, since a skipped run may have no `run_id`.
  - Changes: `changes:<trail index>`.
  - Departures: `departures:<attempt>.<deviation>`.
  - Cost: `cost:milestone:<id>`, `cost:executor:<name>` or `cost:other:<type>`.
  - Health: `health:accept:<task>.<i>`, `health:premise:<task>.<i>` or `health:gate:<milestone>.<i>`.
- **Changes cut-off.** The latest approval is the one with the greatest parsed `at`. A change whose `at` is empty or does not parse is left out, because it cannot be placed after that approval.
- **Milestone goal.** It is the checkpoint's `rationale`, as the required means says. Fix rounds are the number of distinct rounds above 0. The last gate outcome is taken from the highest `(round, seq)`, or reads `not gated`.
- **Duplicated formatters.** `duration` and the token count are written locally, because `view/details.rs::duration` and `tokens` are private to a file this task does not hold.

## Checks

- `rustfmt --edition 2024 --check glimpse/src/surface.rs glimpse/src/plan_state.rs`: clean.
- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`: compiles with no errors. There are 3 dead-code warnings, all in files this task does not hold: `authored.rs` `by_path`, `model/plan.rs` `latest_report`, and `PlanIndex::{gate_rounds_for, health}`.
- Accept check "the plan state and its row type exist" (worker): passes.
- Accept check "the plan state carries its own tests" (worker): passes.
- Accept check `cargo test --manifest-path glimpse/Cargo.toml plan_state`: belongs to the gate, not run here. Every test name starts `plan_state_`, so the filter selects them.

## Handover

None.

TANGENTIAL: debt | glimpse/src/model/plan.rs | `PlanIndex::health` drops a non-probeable entry with a recorded bad verdict and uses a good verdict per kind; the Plan surface's Health section follows the design's rule instead, so the two disagree and `PlanIndex::health` has no caller | two health rules for one design term can drift further; one should go or match the other
TANGENTIAL: debt | glimpse/src/view/details.rs | the duration and token formatters exist privately in view/details.rs, view/activity.rs and now plan_state.rs | three copies of one display rule can drift apart
