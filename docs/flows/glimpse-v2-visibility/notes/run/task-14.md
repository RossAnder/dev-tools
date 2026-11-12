Status: done — twelve glimpse module files created and declared.

Changes:
- glimpse/src/main.rs: added `mod authored;`, `mod nav;`, `mod plan_state;`, `mod rollup;` in alphabetical places.
- glimpse/src/view/mod.rs: added pub(crate) mod doc, doc_view, links, plan, plan_details, task_spec.
- glimpse/src/source.rs: added `mod doc;` before `mod mode;`.
- glimpse/src/model.rs: added `pub(crate) mod plan;` before the `use` block.
- Twelve new files, each a single `//!` line.

Deviations: none.

Checks: rustfmt --edition 2024 --check on all twelve new files, clean. Both accept checks (grep) pass. No cargo clippy run: the edits are module declarations over doc-only files.

TANGENTIAL: none
