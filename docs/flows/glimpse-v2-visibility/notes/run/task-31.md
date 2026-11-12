Status: done — the Inbox opens with a read-only `needs you` section: the viewed flow's parked tasks, open handovers and the latest report's entries that need a person, each leading to its record through `App::open`.

## What changed

- `glimpse/src/surface.rs`: `NeedRow { id, kind, key, text, target }`, `NEED_PREFIX` and `needs_of(&Snapshot)`. The builder returns nothing without a plan store. It lists parked tasks (`need:task:<id>`, target `Task`), then open handovers (`need:handover:<id>`, target `Handover`), then the latest report's entries whose kind is not `parked`/`handover` and whose `needs_person` is true (`need:report:<entry index>`). A report entry's target is a `Source` for a non-empty path, else a `Task` for a non-zero task, else none. `InboxState` holds `needs: Vec<NeedRow>` beside `tracking`. `set_needs` swaps them in and repositions a cursor whose need row vanished, or an unset cursor. `need`/`cursor_need` look them up. `visible()` puts a `needs you` header and the rows first. `apply_inputs` leaves a cursor on a live need row alone. `CursorList::select_row`/`cursor_row_id` accept need ids. The rows never enter `Tracking`, so they never flash, never count toward `new_since_view`, never take a saving mark, and `cursor_row()`, `cursor_question()` and `cursor_withdrawable()` return nothing on them. Tests: the fixture's rows in order with their targets, the report-target fallbacks, and the section leading the Inbox while keeping its cursor across a refresh, with no arrivals.
- `glimpse/src/app.rs`: `rebuild_needs()` runs in `App::new` (for `--once`) and in `apply_snapshot`. `Enter` (`Action::Details`) on a need row calls `App::open(target)`; when there is no target the footer says `this entry names nothing to open`. `Withdraw`, `OpenRequest`, `OpenMenu`, `OpenClassify` and `ToggleMark` on a need row change nothing and the footer says `a needs-you row is read-only`. `inbox_unanswered` adds the need rows to the one count. `y` on a need row copies its task or handover id. The existing Inbox tests now run on a plan-less snapshot (`planless_app`). New tests: rows first in order, Enter opening a task, a handover and a source and Backspace (`GoBack`) returning to the row, a targetless entry, the write actions, the count with no arrival on refresh, and a plan-less flow showing no section.
- `glimpse/src/view/inbox.rs`: `need_line` draws a need row as the status glyph `⊘` for a parked task or the existing `·` otherwise, then the id column (the task or handover id), the kind, and the one-line text in the records' text column. The column widths take the need rows into account. Existing tests use a plan-less snapshot. New tests: the section drawn first with its heading, and a plan-less Inbox drawn as before.
- `glimpse/src/view/header.rs`: only the `tab_count` doc comment and `apply_inputs_updates_the_inbox_badge`. That test now checks the plan-less counts it checked before, plus the fixture case: `Inbox 3?`, then `Inbox 4? +2` and compact `In4?+2 Pl4`.
- `glimpse/src/view/mod.rs`: only `an_item_surface_replaces_the_view_and_records_its_rows`. Before the input read, the Inbox now shows the `needs you` section with three hit targets. After the read it holds four, and the test selects `I1` before checking the question's row and the `enter answer` hint.

## Deviations

None from a suggested means; all three means are required and followed. Two choices the detail left open:
- The count keeps the existing `n?` form and stays hidden until the input store is first read, as before, so the tab gets no new number or mark and the compact-header test's unread `In` is unchanged.
- A needs-you row draws the existing `·` glyph rather than a new mark, since `view/legend.rs` is outside this task.

## Checks

- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`: clean apart from one existing `method health is never used` warning in `glimpse/src/model/plan.rs`, a file this task does not hold.
- `rustfmt --edition 2024 --check` on `app.rs`, `surface.rs`, `view/inbox.rs` and `view/header.rs`: clean after one `rustfmt --edition 2024` apply. `view/mod.rs` has `mod` children, so it is left to the gate; the lines added there are short.
- Accept `grep -q "needs you" … && grep -q "need:" glimpse/src/surface.rs`: rc 0.
- Accept `grep -q "fn [a-z_]*needs_you" …`: rc 0.
- `cargo test --manifest-path glimpse/Cargo.toml inbox`: Run by gate, not run here.

## Handover

None.

TANGENTIAL: annoyance | glimpse/src/view/footer.rs | On a needs-you row the Inbox footer still offers `enter details`, but Enter opens the row's record | The hint misdescribes the key on those rows; `inbox_hints` could read `enter open` when `app.inbox.cursor_need()` is set
TANGENTIAL: debt | glimpse/src/view/legend.rs | The legend's inbox line lists only the input statuses and does not mention the needs-you section or its parked glyph | A user reading the legend cannot tell what the first Inbox section is
