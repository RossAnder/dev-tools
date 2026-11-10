Status: done — `fix_shape` left the finding catalogue; `blocked_on` and its five detail fields replace it, enforced by the validator and stated in the agent schema.

## What changed

- `tomlctl/src/review/fields.rs`
  - New word enum `BlockedOn` (`decision`, `evidence`, `measurement`, `alternatives`, `design`).
  - Catalogue: the `fix_shape` entry is removed. New defect-and-cleanup fields, no family restriction (so the plan family uses them too): `blocked_on` (enum, optional), `blocked_missing` (required for every block), `blocked_options` (StrList, required for decision and alternatives, at least 2 entries), `blocked_metric` and `blocked_command` (required for measurement), `blocked_contract` (required for design).
  - New `Need::WhenBlockedOn(&[BlockedOn])`, following the `WhenChallenge` idiom. Its refusal reads "`<field>` is required when blocked_on is <block>".
  - New `FieldSpec::min_items` with a `min_items()` builder. `check_value` refuses a short list with "has N entries; at least M are required".
  - A supplied `fix_shape` gets its own refusal through a small `RETIRED` table: "is retired; a fixable finding omits `blocked_on`, and one that cannot be fixed as written names what it lacks in `blocked_on`".
  - New helper `blocked_on(&store::Finding) -> Option<BlockedOn>`. It reads `blocked_on` from the row's `rest`. When that is absent and the row has `fix_shape = "needs-plan"`, it answers `Design`. It is `#[allow(dead_code)]` until the select, list, digest and schedule readers call it.
  - Visibility `BLOCKED` = verifier, implementer, orchestrator, audit and report read it. `fix` stays withheld from the verifier.
  - `FIX_SHAPES` is left in place as the detail says, because query.rs still reads it.
  - Unit tests: the defect fixture drops `fix_shape`. New tests: each block's required detail (defect in the code family, cleanup in the plan family); a single option and a supplied `fix_shape` are refused; a legacy needs-plan row reads as design. The verifier projection test now asserts that `blocked_*` is shown. The matrix has rows for the six new fields.
- `tomlctl/src/review/schema_out.rs`: `minItems` from `min_items`. A `WhenBlockedOn` field becomes an `if blocked_on in [...] then required` rule, plus a description note. New unit test `schema_and_validator_agree_on_blocked_detail`, which compares the validator's demanded set with the schema's `then` set for each kind and block, and also checks `minItems: 2` and that `fix_shape` is not offered. The schema keeps `additionalProperties: false`, so it refuses `fix_shape` just as the validator does.
- `tomlctl/src/review/add.rs`: no edit needed. `review add` and `review triage --convert` both reach `fields::validate_row` (through `add::field_refusals` / `field_refusals_with`), so both enforce the new rules.
- `tomlctl/tests/review_add.rs`: the defect fixture drops `fix_shape`. Three new tests match the accept filter `test(blocked)`: `a_blocked_finding_without_its_detail_is_refused`, `a_blocked_finding_with_its_detail_lands`, and `a_blocked_decision_with_one_option_or_a_fix_shape_is_refused`.
- `tomlctl/tests/review_schedule.rs`: the `Defect.fix_shape` field becomes `blocked`, a raw TOML line that is empty for a fixable defect. The refused-selection case keeps a legacy `fix_shape = "needs-plan"` row, because schedule.rs still reads `fix_shape` until its own task moves it to the helper. The legacy row stays valid under the helper either way.
- `tomlctl/tests/{review_apply,review_plan_records,plan_approve,output_options,plan_view,review_reconcile}.rs`: removed `fix_shape = "edits"` / `"fix_shape": "edits"` from the fixtures. An omitted `blocked_on` now means fixable.
- `tomlctl/tests/{review_anchor,review_reproduce,review_store_v2,review_stats}.rs`: none of these files carries `fix_shape`, so they are unchanged.

## Deviations

- The plan sets no caps or item maximum for the detail fields, so I chose them: `blocked_missing` 400, `blocked_options` 300 per entry with at most 6 entries, `blocked_metric` 200, `blocked_command` 300, `blocked_contract` 600. These follow the caps of comparable prose fields (`counter` 300, `failure_scenario` 600).
- The two-option minimum applies whenever `blocked_options` is supplied, not only when it is required. A one-element option list has no meaning under any block, and the rule needs no conditional `minItems`, so the schema and the validator agree without one. The alternative was a conditional minimum under decision and alternatives only. That adds an `if`/`then` for `minItems` and accepts a meaningless list elsewhere.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean, no warnings.
- `rustfmt --edition 2024 --check` on every edited file: clean after applying rustfmt to fields.rs, schema_out.rs and tests/review_add.rs.
- Accept "blocked_on is a catalogue field" (`grep -q '"blocked_on"' tomlctl/src/review/fields.rs`): passes.
- Accept `binary(review_add) & test(blocked)` and accept `test(schema)`: both read `Writes build: yes`, so the gate runs them. Not run here.

## Handover

None.

## Tangential

TANGENTIAL: debt | tomlctl/src/review/fields.rs | Stored legacy `fix_shape` values are no longer catalogued, so `review show --for implementer|orchestrator|report` stops projecting them; only `fields::blocked_on` reads them now | A reader that still shows `fix_shape` (query `--fix-shape`, digest, select, schedule) should move to the helper in the same milestone, or the legacy value silently drops out of role views
TANGENTIAL: debt | tomlctl/tests/review_export.rs, tomlctl/tests/review_query.rs, tomlctl/tests/review_digest.rs | These fixtures still store `fix_shape` rows; they load fine, but any byte-level comparison after a write will see `fix_shape` re-ordered to the end of its table now that the catalogue no longer orders it | Expect diffs there if they compare written bytes
TANGENTIAL: debt | claude/skills/review-v2-method | Agent-facing prose describing the finding fields likely still names `fix_shape` | Finders following it will have rows refused until the skill names `blocked_on`
