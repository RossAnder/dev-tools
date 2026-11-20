Status: done — `--external-gate` added to `plan milestone add` and `update`, storing the command once as a gate and marking it in `external_gates`

```toml run-note
files_touched = [
  "tomlctl/src/cli/types/plan.rs",
  "tomlctl/src/plan/milestone.rs",
  "tomlctl/tests/plan_milestone.rs",
  "claude/skills/tomlctl/references/plan.md",
]

[[deviations]]
what = "A patch that replaces the gate list without giving marks (only the review edit of `gates` does this) keeps just the marks whose command is still a gate"
why = "The detail covers the update flags, which always set both lists together; the review-edit path sets `gates` alone and would otherwise leave a mark naming no gate. The other options were to keep the marks unchanged, which leaves dangling marks for the plan check to report, or to clear every mark, which would drop a still-valid mark on any review edit of the gates. Filtering is easy to undo and changes nothing for a milestone with no mark"
means = "detail: update replaces both lists with what the call gives"

[[deviations]]
what = "The two `MilestoneSpec` literals in plan/milestone.rs already carried `external_gates`, from the schema task's edit; add now fills its literal from the flags and the patch literal keeps an empty list"
why = "An earlier task had already added the field to both literals so the crate compiles"
means = "detail: fill the two MilestoneSpec literals"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/milestone.rs tomlctl/tests/plan_milestone.rs tomlctl/src/cli/types/plan.rs"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"

[[checks]]
command = "grep -q 'external-gate' tomlctl/src/cli/types/plan.rs"
result = "pass"

[[checks]]
command = "grep -q -- '--external-gate' claude/skills/tomlctl/references/plan.md"
result = "pass"
```

## What changed

Both `after:` premises held at d840a59b before any edit.

- `tomlctl/src/cli/types/plan.rs`: a repeatable `--external-gate <CMD>` on `PlanMilestoneAddArgs` and `PlanMilestoneUpdateArgs`. The `--clear-gate` help now says it empties the marks too and is refused with either gate flag.
- `tomlctl/src/plan/milestone.rs`: `gates_and_marks` appends each external command after the `--gate` commands, in the order given, unless it is already there, and marks each one once. Plain `--gate` commands are kept as given, so a call without the flag stores exactly what it stored before. On `add`, the gate probes are checked against the merged list, so a probe may name an external gate. On `update`, either gate flag or `--clear-gate` replaces both `gates` and `external_gates`, and a call that passes neither keeps both. `--clear-gate` with `--external-gate` is refused as a validation error. `MilestonePatch` gains `external_gates`, and `patch_milestone` appends an audited `[[changes]]` row with `field = "external_gates"` and compact-JSON `old`/`new`, only when the marks changed. The output is still `{"ok":true,"id":...}`.
- `tomlctl/tests/plan_milestone.rs`: three tests:
  - `add` stores a command given both ways once and marks it, keeps the order, and accepts a probe naming an external gate. A milestone with no mark writes no key.
  - `update` replaces both lists, writes an audit row, and with `--gate` alone leaves no mark, so the stamp drops back to 2. `--clear-gate` empties both.
  - `--clear-gate` beside `--external-gate` is refused and writes nothing.
- `claude/skills/tomlctl/references/plan.md`:
  - The flag is in both synopsis lines and both flag tables.
  - `external_gates` is named in the update section's audited fields and compact-JSON list.
  - The version-4 stamp is noted.
  - The `## plan emit` pointer now names every leaf: `schedule`, `launch-args`, `gate-list`, `reproduce-units`, `lane-check` and `commit-paths`.

I did not run the gate-owned check `cargo nextest run ... -E 'test(/^plan_milestone::/)'`, because it belongs to the gate.
