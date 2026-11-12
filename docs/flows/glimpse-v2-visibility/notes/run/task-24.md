Status: done — details builders return a Doc with its links, and details.rs exposes them through `links(&App)`

## What changed

- `glimpse/src/view/doc.rs`: added `Doc { text: Text<'static>, links: Vec<Link> }` (with `From<Text>` for linkless documents) and `Link { label: String, target: Target }`.
- `glimpse/src/view/details.rs`:
  - `content` (task) and `item_content` (ledger row) now return `Doc`. A task's links are its Needs, Coupling, Dependents and Shares-files-with peers, in that section order, each a `Target::Task` labelled `#<id> <title>`. A ledger row's links are the pending inputs drawn under Inputs, each a `Target::Input` labelled `<id> <kind>  <text>`.
  - `render` goes through a new private `shown(app, now) -> (Option<String>, Doc)`, which picks the record the same way the old inline match did. It draws `doc.text` the same as before.
  - New accessor `pub(crate) fn links(app: &App) -> Vec<Link>`, a pure function over `&App` that returns the shown record's links. It carries `cfg_attr(not(test), allow(dead_code))` because nothing outside the tests calls it yet; the links list task will. `nav::Target` uses the same attribute.
  - Tests: the `plain` helper takes `&Doc`, and one closure reads `.text.lines`, to fit the new return type. No other existing test changed. Added `a_tasks_links_name_its_related_tasks_in_text_order` (task 5 gives [1, 2, 8, 7, 4] and the accessor matches), `a_rows_links_name_its_pending_inputs`, and `a_record_without_relations_has_no_links` (an empty row and no selection).

## Deviations

- A peer task listed in more than one section (for example coupled and also sharing files) gets one link, at its first mention. The text still shows it in every section. Listing a duplicate would put the same destination twice in the links list.
- A peer id missing from the store is drawn as `? <id>` but gets no link, since `App::open` could not follow it.
- The detail asks for `render` to "hand the links to the app". `render` takes `&App` and cannot write to it, so I did what the detail itself proposes: a pure accessor, `links(&App)`, that rebuilds the shown Doc. Both functions pick the record through the same `shown` function.

## Checks

- `rustfmt --edition 2024` on both files: applied, and nothing left to report.
- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`: compiles, with no warnings in doc.rs or details.rs. The dead-code warnings that remain are in `model.rs` and `model/plan.rs`, files sibling tasks are working on.
- Accept "Doc and Link types exist and the builders use them" (worker): pass.
- Accept "a test covers a record's links" (worker): pass.
- Accept `cargo test ... details` is run by the gate and I did not run it.
- After-premises (details render signature; `enum Target` in nav.rs): both rc=0.

## Handover

None.

TANGENTIAL: none
