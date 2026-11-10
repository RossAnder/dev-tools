Status: done — a verifier verdict with `unblocks` and `fix_sketch` now clears a finding's block and leaves the sketch on the finding

## What changed

- `tomlctl/src/review/fields.rs`
  - New `Need::WhenUnblocks`. The verdict row gets `unblocks` (bool) and `fix_sketch` (string, capped at 800 characters, required when `unblocks = true`).
  - The finding gets two computed fields: `fix_sketch` (FIX visibility, so the implementer and the report see it) and `unblocked_by` (STATUS visibility, values `verifier|user|settle`). Neither is a status companion, so both survive later status moves.
  - New `UNBLOCKED_BY`, `unblocks(row)` and `clear_block(finding, keep)`. `clear_block` drops `blocked_on`, every `WhenBlockedOn` detail field not listed in `keep`, and a legacy `fix_shape = "needs-plan"`, which `blocked_on()` would otherwise still read as a design block. The `keep` parameter is there so the later triage-answer path can keep `blocked_options`.
  - `validate_row` refuses a verdict with `fix_sketch` unless it also has `unblocks = true`.
  - I added the four new fields to the projection matrix test.
- `tomlctl/src/review/verdict.rs`
  - The single form maps `--unblocks`, `--fix-sketch` and `--fix-sketch-file` onto the row.
  - `apply_one` refuses `unblocks` in three cases: `by` is not `verifier`, the verdict is `refuted`, or the finding is not blocked. A replay whose verdict is already stored is exempt from the not-blocked check, so a replay stays idempotent.
  - On landing, the verdict stores `unblocks`/`fix_sketch` in its `rest`. The finding has its block cleared and gets `fix_sketch` and `unblocked_by = "verifier"`. The return object echoes `unblocks` and `fix_sketch`.
  - A verdict without `unblocks` takes the same path as before.
- `tomlctl/src/review/schema_out.rs`: the verdict-return schema gains `unblocks` and `fix_sketch`, both built from the catalogue specs, plus `allOf` rules generated from `Need::WhenUnblocks`: `unblocks: true` requires `fix_sketch`, and `fix_sketch` requires `unblocks: true`. New unit test `verdict_return_and_validator_agree_on_unblocking`.
- `tomlctl/src/cli/types/review/verdicts.rs`: new `--unblocks`, `--fix-sketch` and `--fix-sketch-file` flags, each conflicting with `--ndjson`.
- `tomlctl/tests/review_verdict.rs`: new fixture with a finding blocked on evidence and one using the legacy needs-plan form. Six new tests cover:
  - the unblock itself, including a replay that writes nothing;
  - the legacy needs-plan form;
  - the refusal without `fix_sketch`;
  - the refusal on an unblocked finding;
  - the refusal from a refuting or non-verifier verdict;
  - a verdict without `unblocks`, which leaves the block and refuses a lone `fix_sketch`.

## Deviations / choices

- I added two refusals the constraints do not list: `unblocks` from any `by` other than `verifier`, and `unblocks` on a `refuted` verdict.
  - Why: the design table says "a verifier verdict", and refuting a finding while handing it a fix sketch contradicts itself.
  - Alternative: accept them silently. The verdict would then be recorded with no coherent effect, which is the outcome `review verdict` already refuses for other mismatched verdicts.
- A lone `fix_sketch` (without `unblocks = true`) is refused, not ignored, so a sketch is never dropped silently. The schema states the same rule.
- These rules live in `verdict.rs` because they depend on the store or on `by`, which the verdict-return schema does not carry. The validator/schema pair covers only the `unblocks` and `fix_sketch` pairing, which the constraint "the validator and the generated verdict-return schema agree" is about.

## Handover

None from a person's side beyond the docs follow-up below.

## Checks

- Compile: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` is clean (no warnings or errors).
- Format: ran `rustfmt --edition 2024` on each edited file. Running it on `tests/review_verdict.rs` also walked `tests/common` (`mod common;`), and `git status` confirms nothing there changed.
- Accept check "the verdict row has an unblocks field": `grep -q 'unblocks' tomlctl/src/review/verdict.rs` passes.
- Accept check "binary(review_verdict) & test(unblock)" builds (`Writes build: yes`), so it is the gate's to run and I did not run it. All six new tests have `unblock` in their names. The new fields.rs matrix rows and the schema_out unit test are lib tests, also left to the gate.

## Tangential

TANGENTIAL: debt | claude/skills/tomlctl/references/review.md | the `review verdict` flag table does not yet list `--unblocks`, `--fix-sketch` or `--fix-sketch-file` | the flag-table lint checks only doc→CLI, so the gap will not fail a gate and verifiers will not learn the flags
TANGENTIAL: debt | claude/skills/review-v2-method/SKILL.md | the verdict-return object description and the verifier procedure (claude/agents/review-verifier.md) do not mention `unblocks`/`fix_sketch` | a verifier cannot answer a blocked claim until its method text says how
TANGENTIAL: annoyance | tomlctl/src/review/verdict.rs | `--rationale -` and `--fix-sketch -` both read stdin, so passing both leaves the second one empty | a confusing refusal or empty field if anyone combines them
