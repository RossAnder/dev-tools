Status: done — `tomlctl::review_finding(root, store, id)` reads one v2 review finding through the `report` projection

## What changed

- `tomlctl/src/review/facade.rs` (new): `finding(root, store, id)`. It resolves the repo-relative store path. A path that is empty, absolute or holds any component other than a normal name or `.` is refused, and so is one whose real path is not strictly below `<root>/.claude`; both comparisons use the canonical form. An absent store gives the review group's own `review store not found at <path>` error. The store is loaded with `verify_integrity` and `strict_read` both off. An unknown id is a `NotFound` error naming the store and the id. The finding comes out as `View::project(Role::Report, index, true)`.
- `tomlctl/src/review/mod.rs`: one added line, `pub(crate) mod facade;`.
- `tomlctl/src/lib.rs`: `pub fn review_finding`. It silences advisories, as the other facade reads do, then calls the facade.
- `tomlctl/tests/library_facade.rs`: a `REVIEW` fixture store and three tests:
  - The finding reads back with its summary and status, and equals `review show F1 --for report --scope facade`'s `items[0]`.
  - An unknown id and an unknown store each give an error naming them.
  - A store outside `.claude` is refused, whether given as a sibling path, a path through `.claude/..`, a path starting with `..`, or an absolute path.

## Deviations

- "Reuse the projection `review show` builds": `show` builds each finding through the private `query::finding_row`, which this task could not expose without editing `query.rs`. For the `report` role, `finding_row` adds nothing on top of `View::project(role, index, true)`: `ruled_out` is only for verifier and audit, `excerpts` only for audit, and the backlog reshape only for backlog. The facade therefore calls `View::project` (`pub(super)` in `review/store.rs`) directly. No second projection is built, and the test checks the facade's output for byte-equality against the CLI's output. The other option was to make `finding_row` `pub(super)`, which means editing `query.rs`, a file this task does not hold.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean, with no warnings or errors.
- `rustfmt --edition 2024 --check` on `review/facade.rs` and `tests/library_facade.rs`: clean once the reported wrap in the test file was applied.
- Worker accept checks: the three `grep -q` checks all pass.
- I ran the fixture store through the installed `tomlctl review show F1 --for report` in a scratch repo. It parses and prints the expected item.
- `cargo nextest ... -E 'binary(library_facade)'` is marked `Run by: gate`, so I did not run it.

## Handover

None.

TANGENTIAL: none
