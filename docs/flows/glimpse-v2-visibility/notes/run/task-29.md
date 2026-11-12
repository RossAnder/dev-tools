Status: done — a source, commit, finding or authored file opens in the details panel in place of the row's details, a source at its marked line; Backspace returns to what was under it

## What changed

- `glimpse/src/nav.rs`
  - New types: `OpenDoc { target, over, scroll }` (the document opened over a row), `DocLane { wanted, state }` (the request last sent to the poller and its answer) and `DocState` (`Loading`, `Text(Fetched)` or `Failed(reason)`).
  - `Location` gains `doc: Option<Target>`.
  - `open` now returns `Option<Action>`.
    - `Source`, `Commit` and `Finding` open a document over the present row. Opening one also opens the details panel and pushes the location. On Tasks it pauses follow.
    - `Url` posts `copied <url>` and hands back `Action::Copy`.
    - A row target drops any open document, or keeps it if the target does not resolve. An `Authored` target also opens the details panel.
  - `go_back` restores the stacked document.
  - New: `open_doc`, the document shown only while the surface and cursor are still the ones it was opened over. `doc_target`, the open document, else the authored file under the Plan cursor while details are open.
  - New: `wanted_doc`, `doc_state` (loading unless the lane's request is this target's), `follow_doc` (drops a document the cursor left and returns the request to send when the wanted one changed), `doc_answer` (drops an answer for any other request, and scrolls a source to its line) and `scroll_doc`.
  - Tests: the old "cannot be shown yet" test now checks that source, commit and finding open as stacked documents and Backspace walks them back. A new test checks that a URL is copied and moves nothing.
- `glimpse/src/app.rs`: new `doc` and `lane` fields.
  - The open document is dropped wherever the back stack is cleared on a flow or scope change.
  - `follow_link` and `apply_links` hand back what `open` returns, so a URL link reaches the runtime's copy path.
  - `scroll_details` scrolls the open document's own scroll first.
- `glimpse/src/runtime.rs`: `Screen.doc` and `Document` are removed. The lane lives on `App`.
  - `want_doc(app, host)` sends `app.follow_doc()`'s change. It runs before the first draw and after every batch, as `follow_tail` does.
  - `Event::Doc` goes to `app.doc_answer`.
  - The `allow(dead_code)` attributes on `want_doc` and `Host::set_doc` are gone.
  - The stale-answer test now opens two commits through `App::open`.
- `glimpse/src/view/doc_view.rs`
  - `content(app, target, width)` returns the border title and a `Doc`. The title is the fetched title plus each notice joined by ` · `, or the target's name while it is loading or has failed.
  - Loading shows one `loading…` line, and a failure shows its reason in one line.
  - An authored markdown file goes through `markdown::to_text_within` at the panel's inner width. Anything else is plain text with the cited line drawn in `theme.selection`.
  - Links:
    - An authored file links what its Plan row links: its sources, the run note's task, and the decisions citing it.
    - A commit links the tasks stamped with it. Either sha may be abbreviated.
    - A finding links the tasks whose `finding` names its store and id under fixes or settles.
  - `scroll_to` puts the line a third of the way down the last-drawn page.
  - Six tests, listed under Checks.
- `glimpse/src/view/details.rs`: `render` draws `doc_view::content` at the open document's scroll whenever `doc_target` is set. `links` returns the document's links in that case. The `allow(dead_code)` on `links` is gone.

## Deviations

- Where the document's scroll lives. `view/mod.rs` (not this task's file) passes `details_scroll` into `details::render` and clamps it. `OpenDoc` holds its own `scroll`, and `render` uses it in place of the argument. `scroll_details` routes to it, clamped against the bounds the last frame recorded, which are the document's while it is shown. This means Backspace finds the note's scroll unchanged. An authored-row document has no `OpenDoc` and uses `details_scroll`, because the row's details are not shown alongside it.
  - The other way was to overwrite `details_scroll` while a document is open and save the old value in `Location`. It was rejected because it makes the location compare unequal on scroll alone.
- The jump to a source's line is worked out when the answer arrives, not at draw time. `details::render` takes `&App` and cannot write state back. The jump uses the details rect and page of the last frame. With no frame drawn yet it falls back to no wrapping, so the scroll is `line - 1`.
- A document is tied to the row it was opened over. Moving the cursor or the surface hides it, and the runtime's `follow_doc` drops it and withdraws its request. The design does not say what moving off a document does. Without this, a commit would stay on screen under a different task.
- `open` returns `Option<Action>` so a `Url` can go through the existing copy path (`Action::Copy` handed back to the runtime). The design asks for the copy path but names no signature.

## Handover

None.

## Checks

- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`: compiles. Its two dead-code warnings are in `glimpse/src/model/plan.rs` (`latest_report`, `health`), which this task does not hold.
- `rustfmt --edition 2024 --check` on doc_view.rs, details.rs, nav.rs, runtime.rs and app.rs: clean after applying it.
- Narrow tests `cargo test --manifest-path glimpse/Cargo.toml --bin glimpse -- nav:: runtime::tests:: view::links:: view::details:: view::plan_details::`: 71 passed. These cover the changed `open`/`go_back`, the links list's follow path and the rewritten runtime stale-answer test.
- Accept, the document view draws through the details panel (grep): pass.
- Accept, the document view carries its own tests (grep): pass.
- Accept `cargo test --manifest-path glimpse/Cargo.toml doc_view`: not run, since its `Run by:` is the gate. The six `doc_view_*` tests compile under clippy `--all-targets`. They cover:
  - a source opens loading, then at its line with the line marked and the notice in the title;
  - Backspace returns to the note;
  - an authored row shows rendered markdown with a wrapped table;
  - a failure shows its reason;
  - a commit links its tasks and a finding its fixing task;
  - a late answer is not shown;
  - `b` on an open note lists its source and the citing decision.
- `app.rs` is also in this launch's siblings list, held by a task running alongside this one. Every edit to it was a small anchored `Edit`.

TANGENTIAL: debt | glimpse/src/source/doc.rs | `DocRequest` and `Fetched` still carry `#[cfg_attr(not(test), allow(dead_code))]`, as do `Control::Doc`/`Source::set_doc` in source.rs and `Link` in view/doc.rs; all are now used outside tests | stale lint suppressions hide a future dead-code regression
TANGENTIAL: debt | glimpse/README.md | the README does not yet say that Enter on a link to a source, commit or finding shows it in the details panel, or that a URL link copies the address | users learn keys and behaviour there
