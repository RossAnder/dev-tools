Status: done — five output-options cases in place, and `review anchor` / `review reproduce-status` now print through the read emitter

## What changed

`tomlctl/tests/output_options.rs` (kept from the earlier attempt, unchanged in this run):

- `REVIEW_STORE` fixture gains `F3`, a code defect `scheduled` to `apply-fixture` task `retired-fix` (a ref no row carries), and one `[[reproductions]]` row giving `F1` a `gone` verdict; `schema_version` is 2, the stamp the store gives itself once it holds those rows.
- Five cases sit beside the other review cases: `review reproduce --ndjson -` (write, key `ok`), `review schedule F1 --flow apply-fixture --checkpoint A` (write, key `flow`), `review reconcile --flow apply-fixture` (write, key `flow`), `review anchor F1` (read, `One`, key `id`) and `review reproduce-status F1 --expect gone` (read, `One`, key `expect`). No existing case changed.

`tomlctl/src/review/anchor.rs`: `anchor::run` prints its report with `print_json_line` in place of `print_json_compact`; the import follows.

`tomlctl/src/review/reproduce.rs`: `reproduce::status` prints with `print_json_line`; `reproduce::record` is a write and keeps `print_json_compact`. The import now brings in both.

Both emitters write the same compact bytes when no shaping flag is passed (each returns `write_json(v, Style::Compact)` on inert options), so plain stdout is unchanged, and the `std::process::exit(1)` after the print on a failed anchor or expectation is untouched. A bad shaping flag now fails as a validation error, as the other reads do.

## Deviations

None.

## Checks

- `rustfmt --edition 2024 --check` on the three files: clean.
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- All three accept checks are `Writes build: yes`; left to the gate. The earlier attempt ran each new case's options by hand against the installed binary and found the `--where a=b` refusal on the two reads the only mismatch, which this run's emitter change addresses.

TANGENTIAL: none
