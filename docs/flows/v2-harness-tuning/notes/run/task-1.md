Status: done — verdict_return() no longer emits a top-level allOf.

Changed `tomlctl/src/review/schema_out.rs`: removed the rules/allOf from `verdict_return()` (comment updated), rewrote the `schema["allOf"]` reads in `verdict_return_and_validator_agree_on_unblocking` to assert the validator's refusals directly, and added `verdict_return_has_no_top_level_combinator`.

Deviations: none.

Checks: `cargo nextest ... -E 'test(verdict_return)'` 2 passed; clippy clean for this file; rustfmt check clean; accept grep passes.

TANGENTIAL: none
