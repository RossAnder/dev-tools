Status: done — `review apply --user-accepted` writes an advisory-tier defect or cleanup finding that carries `new`

## What changed

- `tomlctl/src/review/apply.rs`, `judge`: `new` is now decoded before the tier match. An advisory-tier row whose kind is defect or cleanup and that carries `new` is accepted as `Tier::Advisory` under `--user-accepted`. Without the flag it is refused with `<id> is advisory tier; apply its edit with --user-accepted once the user accepts it`. Any other advisory-tier row keeps the old `is advisory tier, which is never applied` refusal. The accepted row then goes through the same Validate and Write passes as a cited or user row: compare-and-set on `expect` inside the target store's lock, then `stamp` with `applied.by = "user"` (the existing `by` selection) and `applied.tier = "advisory"`. Question tier, the kind check, the approved-plan gate and the `new`-missing refusal are unchanged.
- `tomlctl/tests/review_apply.rs`:
  - Added the `advisory_edit` helper.
  - Added `user_accepted_applies_advisory_tier_edit`: applies one advisory edit under `--user-accepted` and checks the record, the resolved status, `applied.by = "user"` and `applied.tier = "advisory"`. A second advisory edit on the same record and `expect` is reported in `skipped_stale` and stays open.
  - Added `advisory_tier_edit_refused_without_user_accepted`: the refusal names `--user-accepted`, nothing is written, and the finding stays open.
  - Reworked the advisory half of `advisory_and_user_tier_findings_are_refused_until_accepted`. It asserted that an advisory-tier edit is refused even under `--user-accepted`, which this task reverses. It now pins that an advisory-kind finding is refused under `--user-accepted` (the "never applied" constraint). The user-tier half is unchanged.

## Deviations

None from a `suggested` means; the task named none. Judgement calls:
- "Carries `new`" means the key is present, including a JSON `null` value, because a null `new` is a valid removal edit elsewhere in this verb (see `mechanical_and_cited_edits_are_written_and_stamped`). It does not mean a non-empty string.
- The reworded message is used only when the row could be applied with the flag (defect or cleanup kind with `new`). An advisory-kind row is never told to pass `--user-accepted`.
- `review add` refuses `new` on an advisory-kind row, so the advisory-kind test row is minted without one. The kind check in `judge` is the backstop that test pins.

## Handover

None.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on both edited files: clean.
- `cargo nextest run --manifest-path tomlctl/Cargo.toml --test review_apply`: 17/17 pass. `--lib -E 'test(review::apply)'`: 12/12 pass. These are the task's own narrow tests; the sibling files compiled cleanly at the time.
- Accept: `grep -q 'fn user_accepted_applies_advisory_tier_edit' tomlctl/tests/review_apply.rs` passes.
- Accept: `grep -q 'fn advisory_tier_edit_refused_without_user_accepted' tomlctl/tests/review_apply.rs` passes.

## Security implication

None beyond the gate this task is meant to relax: an advisory-tier edit now needs the explicit `--user-accepted` flag, the same gate a user-tier row already has. Status, kind, family, target, the target validator and the store-lock compare-and-set are all checked as before.

TANGENTIAL: none
