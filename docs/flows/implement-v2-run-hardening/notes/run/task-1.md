Status: done — `plan emit commit-paths --for work|flow` added, with its tests, output-options case and reference

```toml run-note
files_touched = [
  "tomlctl/src/plan/commit_paths.rs",
  "tomlctl/src/plan/mod.rs",
  "tomlctl/src/plan/emit.rs",
  "tomlctl/src/plan/drift.rs",
  "tomlctl/src/cli/types/plan.rs",
  "tomlctl/tests/plan_commit_paths.rs",
  "tomlctl/tests/it.rs",
  "tomlctl/tests/output_options.rs",
  "claude/skills/tomlctl/references/plan-engine.md",
  "claude/skills/tomlctl/SKILL.md",
]

[[deviations]]
what = "`--for` is a plain String validated by the verb (refused `validation` for anything but work or flow), not a clap ValueEnum"
why = "cli/types/plan.rs's module header leaves every `--for` vocabulary to the verbs so a breach is a `kind=validation` refusal, as `plan view --for` does; a ValueEnum would give clap usage prose instead"
means = "detail: --for work|flow"

[[deviations]]
what = "Both forms read one full status (no pathspec) for the changed paths and filter the flow scope by prefix and exact name; only the flow form's ignored read is narrowed to literal pathspecs"
why = "The flow list has to subtract the work list, which needs the full read anyway; prefix filtering gives the same set as the pathspecs without a third git call"
means = "detail: the flow form's status read over the flow pathspecs"

[[deviations]]
what = "A row file the ignore rules hide is left out of `unmatched` as well as `paths`; `rule` is printed as `<source>:<line>:<pattern>`, and output path lists are sorted byte-wise"
why = "Unspecified choices: a hidden file is reported once, under `ignored`; check-ignore -v's line form joins its fields with colons; git porcelain lists tracked changes before untracked ones, so sorting makes the output stable"
means = "detail: ignored and unmatched keys"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check (each edited leaf .rs file)"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^plan_commit_paths::/) | test(/^output_options::/) | test(/commit_paths::tests/) | test(/drift::tests/)'"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"

[[checks]]
command = "grep -q 'CommitPaths' tomlctl/src/cli/types/plan.rs"
result = "pass"

[[checks]]
command = "grep -q 'plan emit commit-paths' claude/skills/tomlctl/references/plan-engine.md"
result = "pass"
```

## What changed

- `tomlctl/src/plan/drift.rs`: the status read is factored into `git_status(root, extra, pathspecs)` and `status_entries` (code, path); `uncommitted_files` calls them with the same flags and the same skip, so `plan drift` and `lane-check` print what they printed before (the drift unit tests pass unchanged).
- `tomlctl/src/plan/commit_paths.rs` (new): the leaf. `--for work` prints `{paths, ignored, unmatched}` from the milestone rows' `files` matched through `normal_path` against an unskipped status read, deletions included; hidden row files come from `git check-ignore -v -z --stdin`. `--for flow` prints `{paths, ignored}`: the flow store directory, `docs/flows/<slug>/notes/run/`, the backlog and inputs pairs and each review store a milestone row's `[[finding_links]]` names outside the flow directory with its sidecar, less the work list; `ignored` comes from a second status read with `--ignored=matching` over literal pathspecs. A negated rule match is not reported as ignored.
- `tomlctl/src/cli/types/plan.rs`: `CommitPaths(PlanEmitCommitPathsArgs)` in `PlanEmitOp`, with `--milestone` and `--for`.
- `tomlctl/src/plan/emit.rs`, `tomlctl/src/plan/mod.rs`: dispatch arm and `mod` line; the store loads through `load_milestone_store`, so an unknown milestone is refused.
- `tomlctl/tests/plan_commit_paths.rs` (new) and its `mod` line in `tests/it.rs`: work list (row file under `docs/flows/<slug>/notes/`, one under the flow store dir, a deletion, an unchanged file in `unmatched`, an ignored row file under `ignored` with its rule); flow list (plan store, run note, backlog and inputs pairs, the linked review store and sidecar, an unlinked store left out, the work list's path left out, an ignored gate log under the flow dir in `ignored`); unknown milestone and unknown `--for` refused.
- `tomlctl/tests/output_options.rs`: the `commit-paths --for work` case beside `lane-check`, `.git()`.
- `claude/skills/tomlctl/references/plan-engine.md`: intro, contents, synopsis and a `## plan emit commit-paths` section with flag table and output keys. `claude/skills/tomlctl/SKILL.md`: the `plan emit` routing row and the plan-engine reference blurb. No capability string was added.
