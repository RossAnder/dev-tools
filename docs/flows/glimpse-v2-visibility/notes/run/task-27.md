Status: done — `b` opens a modal links list of the shown record's links; Enter or a row click follows one through `App::open`, Esc closes it

Premises checked before editing: both held (`b` unmapped in keys.rs; details.rs calls `plan_details::`).

## What changed

- `glimpse/src/app.rs`: new `LinksList { links, cursor, page }` and `App::links: Option<LinksList>`, held the way the selector's state is; `Regions::links` (row rect, index) for mouse hits; actions `ToggleLinks` (`b`) and `OpenLink(usize)` (a row click). `apply` first hands every action to `apply_links` while the list is up: j/k and arrows move, PgUp/PgDn/Home/End page by the drawn page, Enter and `OpenLink` close the list and then call `App::open`, Esc/`q`/`b` close it and do nothing else, and only the runtime-facing actions (Quit, Copy, SetCapture, ToggleCapture, drag and pan moves/ends) pass through. Every other key is swallowed. `ToggleLinks` reads `view::details::links(self)`. With no links it opens nothing and posts "no links from this record". It does nothing while the legend, selector or theme picker is up. A snapshot of another flow drops an open list.
- `glimpse/src/keys.rs`: `KeyCode::Char('b') => Action::ToggleLinks`. While the list is up, the mouse wheel moves its cursor, a left press on a recorded row maps to `OpenLink(i)`, and nothing else reacts. The two tests that asserted `b` maps to nothing now assert the new mapping. Added a mouse test.
- `glimpse/src/view/links.rs`: `render` draws the centred modal with the shared `panel::block`, a cursor mark and the selection style, labels truncated to the area, and a window around the cursor with ↑/↓ more markers, like the selector. It records each drawn row's rect into `app.regions.links` and the page height into the list. Seven tests cover: `b` on task 5 lists its needs first; Enter opens #1 and Backspace returns to #5 with the list closed; `b` with no links says so; Esc closes without moving (details stay open, back stack empty); the list takes j/k and swallows a digit; a click on a row opens it; on the Plan surface D1's links open its bound task 6, and Backspace returns to `decisions:D1`.
- `glimpse/src/view/mod.rs`: `Regions` built with an empty `links`. The links list is drawn after the theme picker and before a form, and it gets the backdrop like the other modals.
- `glimpse/src/view/footer.rs`: while the list is up the footer reads `j/k move · enter open · esc close`, with no `? keys` tail, as the other modals do. `b links` is added to the Tasks hints after enter/J/K, and to the item-surface hints before `r request`, since a ledger row links its pending inputs. Plan already had it. Added one test.

## Deviations

- The plan says the list "takes every key". runtime.rs is not this task's file, so keys still go through `keys::map` and the list takes them through a guard at the top of `App::apply`. It does not use the `overlay_key` path, which is the selector's precedent. Quit (Ctrl+C) and the capture, drag and pan actions the runtime needs still pass through.
- `b` pressed while the list is up closes it, like `s` for the selector.
- `b` opens the list whether or not the details panel is open. The record is the one the panel would show.

## Checks

- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`: clean for these files. Its two dead-code warnings are in `glimpse/src/model/plan.rs` (`latest_report`, `health`), which this task does not hold.
- `rustfmt --edition 2024 --check` on links.rs, keys.rs, footer.rs and app.rs: clean after applying it to links.rs and keys.rs. view/mod.rs check: no diffs.
- Accept "the links list is drawn and b is mapped" (grep): pass.
- Accept "the links list carries its own tests" (grep): pass.
- Accept `cargo test --manifest-path glimpse/Cargo.toml links`: left to the gate, as its `Run by:` line says. The tests compiled under clippy `--all-targets`.

TANGENTIAL: debt | glimpse/src/nav.rs | `#[cfg_attr(not(test), allow(dead_code))]` on `Target` and `App::open`, and the same attribute on `view::doc::Link` and `view::details::links`, can now come off, since the links list uses them outside tests | stale lint suppressions hide a future dead-code regression
TANGENTIAL: debt | glimpse/README.md | the README's key table may need a `b` row for the links list; this task did not hold the README | users read keys there
