Status: done — review stats splits audited and overturned rows by class per lens, probe, run and model, and by-run rows count ceiling overrides

```toml run-note
files_touched = ["tomlctl/src/review/stats.rs", "tomlctl/tests/review_stats.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 tomlctl/src/review/stats.rs tomlctl/tests/review_stats.rs"
result = "pass"

[[checks]]
command = "grep -q 'audit_classes' tomlctl/src/review/stats.rs && grep -q 'checked_cleanup' tomlctl/src/review/stats.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^review_stats::/)'"
result = "not-run"
```

## What changed

`tomlctl/src/review/stats.rs`
- New `AuditClass` (confirmed_defect, refuted_defect, checked_cleanup, other), classified from the finding's kind, the deciding verifier's refuted call and its `by`: a defect goes by its outcome, a cleanup is `checked_cleanup` only when `checker` decided it, and everything else is `other`.
- New `AuditTally`. It holds the totals plus `(audited, overturned)` per class. Its `add` uses the existing `overturn` and `deciding_verifier`, so the totals are computed exactly as before, and its `write` sets `audited`, `overturned`, `audit_overturn_rate` and `audit_classes: {class: {audited, overturned, rate}}` on a row.
- `Tally` (the lens, probe and run rows) and `by_model`'s `ModelTally` now both hold an `AuditTally` in place of their own two counters. Every existing key keeps its value.
- `by_run` rows gain `ceiling_overrides: {count, over}`, read from the run's `ceiling_overrides` list (in `rest`). `over` is the sum of each entry's `planned + added - ceiling`, computed with saturating i64 arithmetic. A run with no list reports `{count: 0, over: 0}`.

`tomlctl/tests/review_stats.rs`
- `CLASSES_STORE` fixture. It has one lens, an opus verifier and a haiku checker, and the audit runs under a separate run's model. It holds two confirmed_defect rows (one overturned), one refuted_defect row (overturned), two checked_cleanup rows (one overturned), one `other` row (a cleanup a verifier decided), and one unaudited defect. Its run U1 carries two ceiling overrides.
- `audit_counts_split_by_class_per_lens_and_per_model` asserts each class's audited and overturned counts, per lens and per model, and that the class sums equal the existing totals. Overturns fall in three classes.
- `stats_by_run_counts_ceiling_overrides` asserts count 2 and over 5 for U1, and zeros for U2.

The narrow nextest check is marked `Run by: gate`, so I did not run it. Clippy over all targets is clean, which includes the new tests.
