Status: done — the review store holds `scheduled` findings with an apply link and a typed `[[reproductions]]` array, stamped version 2 only while it needs to be

## What changed

- `tomlctl/src/review/fields.rs`
  - `FindingStatus::Scheduled` (`"scheduled"`), added last so the `Ord` of the existing statuses is unchanged. It owns no companions.
  - `is_settled` now includes `scheduled`. Triage refuses a settled finding, so it cannot dismiss, defer, park or reopen a scheduled one. `decided_by_verdict`, `is_reopenable`, `is_suppressing` and `is_syncable` leave it out, so a verdict is recorded beside it without moving it, and sync-v1 never touches it.
  - Catalogue entries `apply_flow` and `apply_task` on findings: computed, with the same visibility as `exported_as`. They are not in `ALL_COMPANIONS`, so `set_status` keeps them on a move to `resolved`. They are not in `LINK_FIELDS` either, because `retire_link` drops every entry in that list and the v1-link retirement must leave the apply link alone.
  - New word enums `ReproduceVerdict` (`gone`, `reproduces`, `unsettled`) and `ReproduceBy` (`verifier`, `orchestrator`).
  - The unit tests now cover the new status, the new settled set, the matrix rows for both link fields, and an assertion that neither link field is a companion.
- `tomlctl/src/review/store.rs`
  - `SCHEMA_VERSION = 2` is the newest version the store reads. `BASE_SCHEMA_VERSION = 1` is the default on read and in `Store::default`.
  - `Store.reproductions: Vec<Reproduction>`. A `Reproduction` has typed `finding`, `flow`, `milestone`, `round`, `task`, `verdict`, `by`, `citation` and `at`, and unknown keys ride in `rest`. On read, `finding`, `flow`, `milestone`, `round`, `verdict` and `by` are required. `at` accepts a date or a date string.
  - `Store::holds_v2_rows()` is true when the store has a reproduction row or a scheduled finding. `to_toml` stamps 2 when it is true and 1 otherwise, whatever version was read. A version-1 store always loads, and a version above 2 is still refused.
  - `[[reproductions]]` is its own top-level array. Nothing that reads `[[verdicts]]` (`View`, `severity`, `upsert_verdict`, resolve, verdict, digest) reads it.
  - Unit tests: `version_two_is_stamped_only_while_needed` and `scheduled_to_resolved_keeps_the_apply_link`.
  - The module header describes the new array and the version rule.
- `tomlctl/src/review/resolve.rs`: in `Outcome::for_target`, a `scheduled` target is handled like `open` and `exported`. A confirmed or plausible re-find is a repeat, and a refuted one is contested. The module doc table says the same.
- `tomlctl/tests/review_store_v2.rs` (new): CLI tests over a hand-seeded store.
  - A store with neither a reproduction nor a scheduled finding is written back as version 1.
  - A store with a reproduction is written as version 2, keeps the reproduction, records it as no verdict and moves no status.
  - A store with a scheduled finding is written as version 2 and keeps the apply link.
  - A version-2 store that no longer needs it is written as version 1.
  - Triage refuses to dismiss or reopen a scheduled finding and writes nothing.
  - A verdict leaves a scheduled finding scheduled.
  - A re-find resolves as a repeat and adds the new run to `seen_in`.

## Deviations

None from a `suggested` means; the task named none. Choices I made where the detail left the edit open:
- I kept the apply link out of `LINK_FIELDS` and added no separate constant for it, because nothing reads such a constant yet and an unused one would trip `dead_code`. The alternative was a separate `APPLY_LINK_FIELDS` constant, left until the reconcile verb needs it to clear the link on a return to `open`.
- There is no `upsert_reproduction` or "current reproduction" helper yet. No verb writes or reads the array in this task, and an unused `pub(crate)` method warns. The write and read verbs for reproductions add them.
- Reproductions have no `Record` variant or catalogue entries. They are typed like the nested `signals` and `ruled_out` tables but keep a `rest`, so a later version's keys survive a write.

## Handover

None for a person. The `review.md` reference (`claude/skills/tomlctl/references/review.md`) does not yet list `scheduled`, the apply link fields, `[[reproductions]]` or schema version 2. A later task in this plan holds that file.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on fields.rs, store.rs, resolve.rs and tests/review_store_v2.rs: clean after applying rustfmt.
- Accept `grep -q '"scheduled"' tomlctl/src/review/fields.rs`: pass.
- Accept `grep -q 'SCHEMA_VERSION: i64 = 2' tomlctl/src/review/store.rs`: pass.
- Accept `cargo nextest ... -E 'binary(review_store_v2)'` and `... & test(scheduled)`: `Writes build: yes`, so they are left to the gate and were not run here. Every test whose name the second filter needs contains `scheduled`.

TANGENTIAL: none
