Status: done — the Plan surface is the seventh surface, reached by key 7, by saved state and `--surface plan`, and by plan links; its list answers the cursor, page and section keys and takes no item action

## What changed

- `glimpse/src/surface.rs`: `Surface::Plan` added last in `ALL` (now `[Surface; 7]`), with label `Plan` and key `plan`; new `Surface::short` gives every surface a unique two-letter label (`Ta Rv Op PR Bl In Pl`); `ledger_kind`, `groups` and `sorts` give Plan none. Tests: digit 7 reaches Plan and 8 nothing, short labels are two letters and unique, `plan` is in the fixed saved keys.
- `glimpse/src/keys.rs`: digit range `'1'..='7'`; the digits test maps 7 to Plan and asserts 8 maps to nothing.
- `glimpse/src/app.rs`: `App::plan: PlanState`, built in `App::new` (for `--once`), rebuilt by `rebuild_plan` after every snapshot, after an authored listing whose revision changed, and on every tick while the Plan surface shows; `rebuild_plan` computes the rollups through `rollup::rollup` at the wall clock's present second. A snapshot of another flow resets the state. `list()`/`list_mut()` return the plan state on Plan; `Tab`/`Shift-Tab` (`NextView`/`PrevView`) move by section on Plan through a new `edit_plan` helper, and change the task view elsewhere as before. One guarded arm makes marks, group, sort, header sort, sort direction, columns, closed toggle, menu, classify, queue, request, withdraw and filter do nothing on Plan. `copy_target` has an explicit Plan case copying the row's key (the part after `<section>:`). `needs_tick` has an explicit Plan flash arm and counts Plan with Tasks in the running-agent case; `tick` expires plan flashes. Tests: 7 reaches Plan and 1 to 7 reach their own; the state is built with the app; cursor, page, select and Tab/Shift-Tab move the plan cursor and leave the task view; Tab still cycles views off Plan; item actions do nothing; a running agent keeps the Plan surface ticking.
- `glimpse/src/nav.rs`: `Milestone`, `Decision`, `Contract`, `Handover`, `Run` and `Authored` targets go through `open_plan_row`, which finds the first plan row whose target is the one opened, switches to Plan and puts the cursor on it; a target with no row changes nothing and says so in the footer. The test that listed those six as "cannot be shown yet" now lists only the four row-less targets; new tests open a decision from Review and come back, and open each plan target kind onto its row; unknown decision, run and authored targets join the unknown-id test.
- `glimpse/src/state.rs`: no code change was needed (the surface saves and restores through `Surface::key`/`from_key`); added a test that a saved `plan` surface restores.
- `glimpse/src/cli.rs`: `--surface` help text and its error message name `plan`; parse test for `--surface plan`.
- `glimpse/src/view/mod.rs`: `draw_surface` has a `Surface::Plan => Hits::default()` arm, drawing nothing, so the exhaustive match compiles; the view is another task's.

## Deviations

- `copy_target` copies the key after the `<section>:` prefix (for example `D1` from `decisions:D1`), reading the detail's "copies the row's key" literally; copying the whole row id was the alternative, and is less useful to paste.
- Item-only actions are refused by one explicit guarded arm in `App::apply` rather than relying on `items` having no Plan entry, so a later change to `current_items` cannot route them to Plan.

## Handover

None.

## Checks

- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`: clean apart from three dead-code warnings in `authored.rs` and `model/plan.rs`, files this task does not hold.
- `rustfmt --edition 2024 --check` on app.rs, surface.rs, keys.rs, state.rs, nav.rs, cli.rs: clean. `view/mod.rs` has `mod` children and is left to the gate.
- Accept "Plan is a surface with a short label": pass.
- Accept "the digit keys reach seven surfaces": pass.
- Accept "a test reaches the plan surface by key": pass.
- Accept `cargo test --manifest-path glimpse/Cargo.toml plan`: run by the gate, not run here.

TANGENTIAL: annoyance | glimpse/src/plan_state.rs | the plan state's first build comes from the snapshot alone, so when the authored listing arrives later every Brief and Notes row counts as an arrival and flashes | a v2 flow opens with its notes flashing and, off screen, bumping new_since_view
TANGENTIAL: question | glimpse/src/view/header.rs | `tab_count` falls through to the items map for Plan and shows it as a missing ledger until the header learns PlanState::count | the Plan tab reads as "no ledger" until the header task lands
