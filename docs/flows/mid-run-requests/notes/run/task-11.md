Status: done — plan check warns plan/file-case-collision on a created file whose stem differs from a neighbour's only by case

```toml run-note
files_touched = [
  "tomlctl/src/plan/check/case_collision.rs",
  "tomlctl/src/plan/check.rs",
  "tomlctl/tests/plan_check.rs",
  "tomlctl/src/cli/dispatch/tests/finding_classes.rs",
]

[[deviations]]
what = "A row's own other files count as neighbours, not only other rows' files, so one row creating Foo.ts and foo.tsx also warns"
why = "the same confusion arises whichever row plans the pair; ids then name just that row"
means = "detail: compare against the stems of every other row's planned files"

[[deviations]]
what = "Directories in the listing are not neighbours; paths with a `..` component, a leading `/` or a `:` are skipped"
why = "a directory has no extension to compare and is no file collision; skipping escaping paths keeps the check from listing directories outside the repo"
means = "detail: compare against the stems of the exact names in the same directory"

[[deviations]]
what = "One finding per colliding path pair, its ids merged across both sides, so two pending rows creating Foo.ts and foo.tsx yield one warning naming rows 1 and 2"
why = "each created side would otherwise raise the same pair twice"
means = "detail: emit one Finding per collision"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "fail"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/check/case_collision.rs tomlctl/src/cli/dispatch/tests/finding_classes.rs tomlctl/tests/plan_check.rs"
result = "pass"

[[checks]]
command = "grep -q 'plan/file-case-collision' tomlctl/src/plan/check/case_collision.rs && grep -q 'mod case_collision' tomlctl/src/plan/check.rs"
result = "pass"

[[checks]]
command = "grep -q 'plan/file-case-collision' tomlctl/tests/plan_check.rs"
result = "pass"
```

## What changed

- `tomlctl/src/plan/check/case_collision.rs` (new): `case_collision_findings(tasks, root)`. For each file of a pending row that is created, meaning a `(new)`-kind note or an exact name absent from `fs::read_dir` of its directory (never `Path::exists`), it compares the stem (the name without its last extension) against the on-disk names in that directory (less any path some row marks `(delete)`) and every planned, non-deleted file there. A collision is stems equal ignoring case but spelled differently. Emits `plan/file-case-collision` at WARNING with the creating row and every row planning the other path. Unit tests cover stem, collision and path splitting.
- `tomlctl/src/plan/check.rs`: `mod case_collision;`, the call beside `ui_scope_findings` outside the before-approval block, and the class added to the header's warning list.
- `tomlctl/src/cli/dispatch/tests/finding_classes.rs`: `documented_plan_classes_match_the_source` also reads `src/plan/check/*.rs` (sorted, through `source_bodies`); `source_bodies` and the tasks test are unchanged.
- `tomlctl/tests/plan_check.rs`: six cases. ColourField.tsx on disk + (new) colourField.ts warns with ids [1]; unannotated src/foo.ts beside on-disk src/Foo.ts warns; two rows planning Foo.ts and foo.tsx warn with ids [1, 2]; Button.css beside Button.tsx and colourField.test.ts beside ColourField.tsx are clean; ColourField.tsx marked (delete) + (new) colourField.ts is clean. The base fixture's clean test is unchanged.

## Checks

The clippy run failed with `E0583: file not found for module request` at `tomlctl/src/plan/mod.rs`, which belongs to a sibling task whose `request.rs` is not yet written. No diagnostic named a file this task holds, so this task's code is not compile-checked yet. The `plan_check::` nextest accept check belongs to the gate. The finding-class parity test will pass only once the sibling task documenting the class in `claude/skills/tomlctl/references/plan.md` lands. I did not run that test.
