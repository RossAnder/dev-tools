Status: done — the keyless-entry schema test now blanks the fixture's finding link too, and covers finding links on read and write

## What changed

- `tomlctl/src/tasks/schema.rs`, test `an_entry_with_no_key_is_dropped_on_both_sides`: the read half now also injects a keyless `[[finding_links]]` entry and asserts it is dropped; the write half blanks the `ref` of `v2_fixture()`'s finding link along with the other keyed entries, and asserts `finding_links` is not written.

## Why the test failed, and why the fix is in the test

`v2_fixture()` gained a fully keyed finding link (ref plus two `fixes`). The test blanked the key of every other v2 entry but not that one, so the store legitimately held a surviving v2 key and was stamped 2. The code was already right: `to_toml` filters keyless finding links (`FindingLink::is_empty`) before computing `holds_v2_keys`, and `from_toml` drops them on read. So no code change was needed. The test's expectation (stamp 1) is kept; the fixture is brought in line with the test's intent that every entry is keyless, which is what the constraint asks for.

## Deviations

None. No suggested means was given.

## Checks

- `rustfmt --edition 2024 --check tomlctl/src/tasks/schema.rs`: clean (the file's only `mod` child is the inline `tests`).
- `cargo clippy --manifest-path tomlctl/Cargo.toml --lib --tests`: clean, no warnings.
- Both accept checks are `Writes build: yes` and are left to the gate.

TANGENTIAL: none
