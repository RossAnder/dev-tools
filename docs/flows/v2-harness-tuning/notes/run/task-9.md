Status: done — the cited test now holds for compound citations, paths named in `new`, and documents of the finding's flow

## What changed

- `tomlctl/src/review/tier.rs`:
  - `citation_holds` splits the citation on `;`, trims each part, and holds when any part parses (through the unchanged `parse_citation`), names an allowed path, and that file has at least the cited end line. An allowed path is one `named_paths` returns or one under `docs/flows/<slug>/`. `parse_citation` already refuses `..`, absolute and backslash paths, so the prefix test cannot be escaped into another flow or out of the repo.
  - `named_paths` takes the finding's decoded `new` and walks it with the target's current value, using the same string, `design/<file>.md#<anchor>` and nesting rules.
  - `cited` takes `new` and passes it down; `compute` passes the decoded `new` it already holds.
  - The module header's `cited` tier line and the two functions' doc comments are restated to match.
  - Three new unit tests: `compound_citation_holds` (one part holds gives `cited`, neither part holding gives `advisory`), `citation_of_a_path_in_new_holds` (a path only `new` names gives `cited`, the same citation with a `new` that does not name it gives `advisory`), and `citation_of_a_flow_document_holds` (a document of the flow gives `cited`, a line past its end or a document of another flow gives `advisory`).

The constraints hold. A citation of code outside the target, its value, its `new` and the flow's documents still fails (`src/a.rs` on an `action` target is advisory in `compound_citation_holds`). `stamp` still returns early when `tier` is already set, so a stored tier is never recomputed.

## Deviations

None. The detail's edit was followed as written.

## Handover

None.

## Checks

- `rustfmt --edition 2024 --check tomlctl/src/review/tier.rs`: clean.
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean, no warnings.
- The detail's narrow run, `cargo nextest run --manifest-path tomlctl/Cargo.toml --lib -E 'test(citation)'`: 6 of 6 passed, the three new tests and the existing `a_citation_of_a_path_the_value_names_holds` among them.
- Accept checks (all `Writes build: no`): `grep -q 'fn compound_citation_holds'`, `grep -q 'fn citation_of_a_path_in_new_holds'` and `grep -q 'fn citation_of_a_flow_document_holds'` on `tomlctl/src/review/tier.rs` each pass.

TANGENTIAL: none
