Status: done — milestone B's five review feature tokens are registered, `review recheck-targets` has its output-options case, and the tomlctl references describe the new flags, verbs and fields

## What changed

- `tomlctl/src/cli/types/mod.rs`: `FEATURES` gains `review_fix_files`, `review_addressed`, `review_digest_with`, `review_add_dry_run` and `review_reproduce_checker`, under a one-line group comment like the earlier groups.
- `tomlctl/tests/capabilities.rs`: the expected list gains the same five tokens in the same order.
- `tomlctl/tests/output_options.rs`: a `review recheck-targets --slug <S> --run U1` read case, shape `One`, key `run`, placed after the `digest` case.
- `tomlctl/README.md`: the five tokens are added to the capabilities sample, and the feature table gains one row per token.
- `claude/skills/tomlctl/SKILL.md`: one feature-table row grouping the five tokens, linking both references.
- `claude/skills/tomlctl/references/review.md`:
  - Store shape: `fix_files` on the sample finding, and `checker` in the reproduction `by` comment.
  - `resolved`'s companions now include `addressed`.
  - Field catalogue: a `fix_files` row (at most 8 paths) and `fix_files` in the path rule. `addressed` is listed as computed.
  - `cover`: the cut note and reason, `…(+K)`, `note_truncated` and `reasons_truncated`, and the replay staying `unchanged`.
  - `add`: `--dry-run` (prose, flag row, example) and the cell-return `same_as: true` mark.
  - Tiers: the widened `cited` rule (`;`-separated parts; paths named by `new`; `docs/flows/<slug>/`).
  - `apply`: the `--user-accepted` exception for an advisory-tier defect or cleanup carrying `new`, in the refusal list and the flag row.
  - `digest`: counts by default, with `--with duplicates,ruled-out` (prose, flag row, example).
  - New `### tomlctl review recheck-targets` section. The Contents line and the reading-verbs integrity list name it.
  - `triage`: `--addressed` / `--addressed-file` and `--targets` (flag rows, example, a paragraph on mode `addressed`).
  - `stats --plan-outcomes`: the `author-addressed` disposition.
- `claude/skills/tomlctl/references/review-apply.md`: `schedule`'s grouping now counts `fix_files` among a finding's edited paths, writes `(new)` file notes, and counts those paths toward the lite rule. `reproduce` accepts `by = "checker"`, and the reference notes the verb has no `--by` flag.

## Deviations

- No `suggested` means was departed from; I ran the suggested nextest filter.
- One edit outside the detail's list, inside a held file. `review add`'s prose said only `unverified` rows become `superseded`. The supersede filter in `tomlctl/src/review/add.rs` (`finish` closure) also withdraws the cell's `open` advisory rows, a behaviour milestone A landed. I corrected the sentence because the reference would otherwise contradict the code.
- To stay under the 600-line reference ceiling (`skill_references_under_line_ceiling`), I folded the cut-text and dry-run text into their verbs' existing paragraphs instead of giving each a bold-lead paragraph.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on `tests/output_options.rs` and `tests/capabilities.rs`: clean. I skipped `cli/types/mod.rs` because it has `mod` children; the edit only adds string literals to a list.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: the first run failed on `skill_references_under_line_ceiling` (review.md had 604 lines). After the trim, review.md has 599 lines and the run passes 54 of 54.
- `cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(capabilities) | test(every_leaf_command_has_a_case) | test(every_case_honours_the_output_options::review)'`: 72 of 72 passed.
- Accept checks (all `Writes build: no`): the five-token grep, the `recheck-targets` / `--addressed` grep on review.md, and the `recheck-targets` grep on output_options.rs all pass.

## Handover

None. Run `cargo install --path tomlctl` after landing so the installed binary advertises the tokens; the milestone's deploy step already covers this.

TANGENTIAL: debt | claude/skills/tomlctl/references/review.md | the file has 599 lines against the 600-line ceiling of `skill_references_under_line_ceiling`, so milestone C's reference task will need to split it (for example, move Plan records or Dispositions into their own reference) | any further line fails the dispatch gate for the whole milestone
