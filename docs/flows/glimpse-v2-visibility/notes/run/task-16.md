Status: done — `CursorList` trait over `ItemsState` and `InboxState`, with `App::list`/`App::list_mut`/`App::edit_list` replacing the Inbox-versus-items guard pairs for the cursor operations

## What changed

- `glimpse/src/surface.rs`: new `pub(crate) trait CursorList` with `move_cursor`, `page_cursor`, `scroll_by`, `viewed`, `select_row`, `cursor` (the raw cursor id), `cursor_row_id` (the id of the row under the cursor, `None` when no row carries it) and `unpin` (clears `scroll_pinned`). Implemented for `ItemsState` and `InboxState`, each delegating to its existing inherent method or to `Tracking`. A private `Tracking::select_row` holds the "set the cursor only when the row exists" rule both `SelectItem` arms used.
- `glimpse/src/app.rs`:
  - `App::list(&self)` and `App::list_mut(&mut self)` return the current surface's list as `&dyn CursorList` / `&mut dyn CursorList`: the Inbox on Inbox, the surface's `ItemsState` otherwise, `None` on Tasks (Tasks has no entry in `items`).
  - `App::edit_list` wraps a cursor edit the way `edit_items`/`edit_inbox` do: when the cursor changed, it unpins the list and zeroes `details_scroll`. The order half of those wrappers (unpin when group/sort/closed/filter changed) is not needed here, since no trait operation changes the visible order.
  - Folded: `Page` (two guarded arms into one `surface != Tasks` arm through `edit_list`), `ScrollView` (two arms into one through `list_mut`), `SwitchSurface`'s viewed mark (one `list_mut().viewed()`), `ItemMove` (if/else into one `edit_list`), `SelectItem` (two arms into one), `has_cursor_row` and the non-Tasks half of `copy_target` (through `list()`).
  - Import of `CursorList` added. With the trait in scope, `state.viewed()`/`scroll_by` calls elsewhere now resolve to the trait methods before auto-deref to `Tracking`; the trait bodies call the same `Tracking` methods, so behaviour is identical.

## Arms left with their own guard, and why

- `SortBy(HeaderKey::Inbox)` / `SortBy(HeaderKey::Item)`: column types differ (`InboxColumn` vs `ItemColumn`) and the item arm reports whether the surface can sort by the column.
- `FlipSortDir` and `ToggleClosed`: the two halves differ only in name, but the operations are not among the trait's cursor operations the design names (move, page, scroll, viewed, cursor id, cursor row). Adding them would make every future list, including the read-only Plan list, answer sort and closed toggles it may not have. They stay on `edit_inbox`/`edit_items`, which also carry the order-change unpin these two need.
- `notify_sort`: reads differently typed sort labels per state.
- `Withdraw` (Inbox only), and `ToggleMark`, `MarkVisible`, `CycleGroup`, `CycleSort`, `OpenColumns`, `OpenMenu`, `OpenClassify`, queue, request, filter and the marks branch of `Back`: item-surface-only actions, as the task says.
- `inbox_question` and `apply_inputs`' `viewing` test the Inbox alone, with no items fallback; not a guard pair.

## Deviations

- The design lists "whether a cursor row exists" as a trait operation; it is provided as `cursor_row_id` (from which `has_cursor_row` and `copy_target` both derive), plus a raw `cursor` and `unpin` that `edit_list` needs to reproduce the wrappers' cursor-moved behaviour. An `App::list(&self)` was added beside `list_mut` because `has_cursor_row` and `copy_target` take `&self`.

## Checks

- `rustfmt --edition 2024 --check glimpse/src/surface.rs glimpse/src/app.rs`: clean (after folding one chain to a single line).
- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`: clean, no warnings.
- Accept (worker): `grep -q "trait CursorList" glimpse/src/surface.rs && grep -q "fn list_mut" glimpse/src/app.rs`: exit 0.
- Accept `cargo test --manifest-path glimpse/Cargo.toml app::`: Run by gate, not run. No existing test was edited.

## Handover

None.

TANGENTIAL: none
