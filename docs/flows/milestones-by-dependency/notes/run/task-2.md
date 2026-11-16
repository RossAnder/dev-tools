Status: done — `--after` on plan milestone add and update, `--clear-after` on update, validated against store order.

```toml run-note
files_touched = ["tomlctl/src/cli/types/plan.rs", "tomlctl/src/plan/milestone.rs", "tomlctl/tests/plan_milestone.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^plan_milestone::/)'"
result = "pass (25 tests)"

[[checks]]
command = "grep -qF 'long = \"after\"' / 'long = \"clear-after\"' tomlctl/src/cli/types/plan.rs"
result = "pass"
```

## What changed

- `cli/types/plan.rs`: `after: Vec<String>` on add and update args, `clear_after` on update.
- `plan/milestone.rs`: `check_after` refuses an id not stored earlier in `[[checkpoints]]` (including the milestone itself or an unknown id); add stores the list; update replaces or clears it through `MilestonePatch`/`patch_milestone`, refuses both flags together, counts either as a change, and audits an `after` row with JSON-encoded old/new. `after` is not in `REVIEW_FIELDS`.
- `tests/plan_milestone.rs`: four tests covering add, refusals writing nothing, replace/clear, and the conflicting flags.
