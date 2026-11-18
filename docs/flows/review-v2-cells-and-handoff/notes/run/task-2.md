Status: done — shared harness moved to review_harness.rs, five empty test modules registered

```toml run-note
files_touched = ["tomlctl/tests/review_cells.rs", "tomlctl/tests/review_harness.rs", "tomlctl/tests/it.rs", "tomlctl/tests/review_propose_slices.rs", "tomlctl/tests/review_propose_admission.rs", "tomlctl/tests/review_replay.rs", "tomlctl/tests/review_ceiling.rs", "tomlctl/tests/review_escalations.rs"]

[[checks]]
command = "grep -q mod lines in it.rs and pub(crate) struct Repo in review_harness.rs"
result = "pass"
```

## What changed

- review_harness.rs: SCOPE, VOCABULARY, STORE, lens, check, facet, write_catalogue, Repo (fields and methods pub(crate)), RunSpec, concerns, strings, ceiling_cells, with_concern, ceiling_cut, flat_files, moved verbatim.
- review_cells.rs: helpers removed, imported from crate::review_harness; tests untouched.
- Five new modules with a single `//!` line each; all six mods registered in it.rs alphabetically.

## Not verified

The compile check (`cargo clippy --test it`) could not run: the library fails with E0761 (src/review/cells.rs and cells/mod.rs both exist), a sibling's in-flight split. rustfmt ran on the two edited files. Unused-import warnings in review_cells.rs (e.g. `lens`/`check` imports were not added; `cli`, `git_available`, `git_command`, `sandbox` are still used by its own code) are not confirmed by the compiler. The review_cells nextest check is the gate's.
