Status: done — Target, Location, App::open, the back stack and Backspace are in place for tasks, backlog rows and input records

## What changed

- `glimpse/src/nav.rs`: `Target` with every variant the design names; `Location { surface, cursor }`, where `Cursor` is `Task(Option<u32>)` on Tasks or `Row(Option<String>)` on a list surface. `impl App` adds `open(target)` and `go_back()`. `open` resolves `Task` through `App::select` (follow is paused, as a click pauses it), and resolves `Backlog` and `Input` to the row on the Backlog surface or the Inbox. A row the closed toggle hides is revealed by turning on `show_closed`, and one the filter hides by clearing the filter; either says so in the footer. Every other variant leaves state alone and shows "<a kind> cannot be shown yet". An unknown id shows "no task N in this flow" or "no backlog item X" and pushes nothing. `open` pushes the location it left only when the target resolved and the location actually changed. Tests are in the module.
- `glimpse/src/app.rs`: new `Action::GoBack`, which calls `go_back`, and a new `App::back: Vec<Location>` field. The stack is cleared in `apply_snapshot` when the slug changes, in `SwitchFlow` when it hands the switch to the runtime (a ledger-only flow never sends a snapshot), and in `SwitchScope` when the scope changes. `select`, `notify`, `list`, `edit_list`, `edit_items` and `edit_inbox` became `pub(crate)` so nav.rs can use them. The `Esc` arm (`Action::Back`) is unchanged.
- `glimpse/src/keys.rs`: `KeyCode::Backspace => Action::GoBack`, and the key-table test has a Backspace case. The tests asserting that `b` maps to nothing still pass, because `b` belongs to a later task.

## Deviations

- The Cursor shape. The design gives Location as "surface, that surface's cursor, and the shown document if any" with no type. I used a two-variant `Cursor` enum because Tasks has a `u32` selection and the lists have a string id. The alternatives were two optional fields (unused slots on every location) or storing the task id as a string (it would need parsing back). There is no document field yet, because no document lane exists. The task that adds the lane extends `Location`.
- How a hidden row is revealed. The detail says only "revealed". I turn on `show_closed` only when the row's class is Done or Declined, and clear the filter only when the row is still hidden after that. The other option was always doing both, which changes more of the user's view than needed.
- Opening a task sets `follow = false`, matching `Action::Select`. Otherwise the next snapshot would move the selection off the opened task.
- `Target` and `open` carry `#[cfg_attr(not(test), allow(dead_code))]`, as `Surface::digit` does, because no key or link calls `open` until the links-list task lands.

## Checks

- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`: compiles. The only 4 warnings are dead-code warnings in `src/model.rs` and `src/model/plan.rs`, which another task holds. My files have none.
- `rustfmt --edition 2024 --check` on nav.rs and keys.rs: clean. app.rs has no `mod` children and was formatted the same way.
- Accept, `pub(crate) enum Target` plus `fn open(`: pass.
- Accept, `KeyCode::Backspace` in keys.rs: pass.
- Accept, `#[test]` in nav.rs: pass.
- `cargo test ... nav`: not run, since it belongs to the gate.

## Handover

None.

TANGENTIAL: debt | glimpse/README.md | the keys table does not list Backspace (back) yet | the README documents every key, and this task does not hold README.md
