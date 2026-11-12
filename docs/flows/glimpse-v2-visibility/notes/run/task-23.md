Status: done — the Plan surface is drawn through draw_surface, its tab is counted and labelled, and the footer and legend describe its keys.

## What changed

- `glimpse/src/view/plan.rs`: new list view. `render` draws each section as `▾ <label> (<n>)`, then its rows, or `  none` when the section is empty, through `table::draw_rows`. It returns `items::Drawn` with the row targets, scroll offset and page, as `inbox.rs` does. Cells are aligned within their section; every cell but the last is capped at 24 cells and the last takes what is left. The first cell is plain, the others use `facet` (lifted by `on_cursor`), and a Health verdict or an open handover's state uses `warning`. The cursor row takes `selection` and `selection_mark`, and a flashing row takes `theme.flash`. A flow with no plan store draws `PlanState::empty_text` (`not a v2 flow: it has no plan store`) in `secondary`. Four tests: every section and one row of each at widths 100 and 44, an empty section reading `none`, the cursor row being marked and scrolled to, and the not-v2 line.
- `glimpse/src/view/mod.rs`: the `Surface::Plan` arm of `draw_surface` calls `plan::render` and writes `scroll` and `page` back to `app.plan`. A new test draws the Plan surface in a full frame, checks that its rows are in `regions.items` and that the footer shows `tab section` without `m actions`, and checks that a click selects `handovers:H2`.
- `glimpse/src/view/header.rs`: the Plan tab's count is `PlanState::count()`. Without a plan store the tab is faded as missing. It never shows an arrival badge: `arrival_badge` returns 0 for Tasks and Plan. Compact tabs are labelled by `Surface::short` (`Rv Op PR Bl In Pl`), and the doc comment says so. The existing tests now expect the seventh tab and the two-letter labels. Two new tests: compact labels are distinct, with Plan and Plan-review apart, whether or not the flow has a plan store; and the Plan tab is counted, shows no badge and is faded without a store.
- `glimpse/src/view/footer.rs`: `plan_hints` lists j/k move, PgUp/Dn page, tab section, enter details, b links, bksp back and 1-7 surface. Compact density drops page and surface. Plan joins the `y copy id` surfaces, because `App::copy_target` handles it. All `1-6` hints are now `1-7`. New test: the Plan list names those keys and none of the item hints.
- `glimpse/src/view/legend.rs`: the `1-7` row now names `plan`. Two rows were added: `tab S-tab` → plan next / previous section, and `b  Backspace` → links / back. The test that keeps `KEYS` at 32 rows or fewer still passes because `f` was merged into the `s  a` row and `space` with `V`. `every_bound_letter_key_has_a_legend_row` now also checks `b` and `Backspace`.
- `glimpse/src/view/items.rs`: `empty_text` names Review explicitly and returns an empty string for Tasks, Inbox and Plan, so it no longer falls through to the review wording for them.

## Deviations

- The detail asks for rows "drawn with view/table.rs". plan.rs uses `table::Row` and `draw_rows` but not `table::header`. The sections have different columns, so one column-title row cannot head them all. The other option was a title row under each section heading, which would cost 11 rows on a surface already about 31 rows tall.
- The legend gained its two rows by merging existing ones (`f` with `s a`, `space` with `V`) rather than raising the 32-row cap in `the_key_table_stays_short_enough_to_scan`.
- The footer lists Backspace as `bksp back` and leaves out `q back`, so the "back" hint is the one D15 binds. `q`/`esc` still work on the surface.

## Handover

None.

## Checks

- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`: clean in my files. There are two dead-code warnings in `glimpse/src/model/plan.rs` (`latest_report`, `health`), which is not a file this task holds.
- `rustfmt --edition 2024 --check` on plan.rs, header.rs, footer.rs, legend.rs and items.rs: clean after applying it to legend.rs. A check on mod.rs reported no diff in mod.rs.
- Narrow tests: `cargo test --bin glimpse -- view::plan view::header view::footer view::legend view::tests::the_plan_surface view::items` passed 61 of 61.
- Accept "the plan view draws through draw_surface" (grep): pass.
- Accept "the footer and legend name seven surfaces" (grep): pass.
- Accept "the plan view carries its own tests" (grep): pass.
- Accept "the sections and rows render…" (`cargo test … view::`): run by the gate, not run here.

## Notes

- `b` is still unmapped in `keys.rs`, where a test asserts that it maps to nothing. The footer and legend name it per D15; binding it belongs to a sibling task.

TANGENTIAL: none
