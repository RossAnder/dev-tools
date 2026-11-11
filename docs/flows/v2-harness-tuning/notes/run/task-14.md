Status: done — `checker` is now a valid reproduction `by`.

Changes:
- tomlctl/src/review/fields.rs: added `Checker => "checker"` to `ReproduceBy`.
- tomlctl/tests/review_reproduce.rs: added `reproduce_accepts_by_checker` (records a checker row, reads it back).
- store.rs and reproduce.rs needed no edit: both parse through `ReproduceBy::parse`, and the error text is built from `ReproduceBy::VALUES`.

Deviations: none.

Checks:
- cargo clippy --all-targets on tomlctl: clean.
- rustfmt --edition 2024 --check on the test file: clean.
- grep accept check: passes.
- narrow nextest for reproduce_accepts_by_checker: passes.

TANGENTIAL: none
