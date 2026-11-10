Status: done — formatted tomlctl/tests/review_plan_records.rs with rustfmt

- tomlctl/tests/review_plan_records.rs: ran `rustfmt --edition 2024` (two hunks: `TASK_RECORDS: &str` spacing, and the `plan handover set` call wrapped). Only one such call exists, so nothing was deduplicated.
- No deviations.
- Check: `rustfmt --edition 2024 --check tomlctl/tests/review_plan_records.rs` passes with no diff.
- No compile check run (formatting only).

TANGENTIAL: none
