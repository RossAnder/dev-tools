Status: done — review stats --yield reports per-run and per-depth first, repeat and context-free arms

```toml run-note
files_touched = ["tomlctl/src/review/stats.rs", "tomlctl/tests/review_stats.rs"]

[[deviations]]
what = "A cell counts as repeat when its pass is 2 or more, not exactly 2"
why = "A third pass is still a repeat of the first; no code mints one today, so the outputs agree on every store"
means = "detail: repeat when its pass is 2"

[[deviations]]
what = "A finding is cross-run when its same_as or merged_into names a finding of any other run in the store, not only an earlier one, and it is left out of findings and verified as well as the clusters"
why = "Run order is not needed to tell the links apart, and the detail's test wants such a row counted in cross_run only"
means = "detail: names a finding of an earlier run"

[[deviations]]
what = "applied and scheduled read apply_state on every member of a unique cluster, not only the cluster root"
why = "The task detail says any member; that covers the root and also a merge whose root is not the row apply scheduled"
means = "D15: applied reads apply_state on the cluster root"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/review/stats.rs tomlctl/tests/review_stats.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "grep -q 'long = \"yield\"' tomlctl/src/review/stats.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^review_stats::/)'"
result = "not-run"
```

## What changed

- `tomlctl/src/review/stats.rs`: a new `--yield` flag (field `yields`, since `yield` is a keyword) that counts as a section, so on its own it prints no `rows`. `yields` walks every kept run. A finding's arm comes from its cell: `cf` for `review-finder-cf` (`fields::FINDER_CF`), `repeat` for pass 2 or more, else `first`. `run_yield` clusters the run's findings with a small union-find over `same_as` and `merged_into`, treated as undirected. It credits `unique` to `first` for clusters with a verified first-pass member, and to `repeat` or `cf` for clusters holding a verified member of that arm and no verified first-pass member. A credited cluster counts in `applied` when any member's `apply_state` is `resolved`, and otherwise in `scheduled` when one is `scheduled`. The output is `{"yield": {"runs": [{store, run, depth, first, repeat, cf, cross_run}], "pooled": {"<depth>": {...}}}}`. The section is read-only, and the module header gets one sentence on the rule.
- `tomlctl/tests/review_stats.rs`: `stats_yield_credits_only_clusters_without_a_first_pass_member` builds a three-run store and checks each case the detail lists: a same_as repeat adds nothing unique, a first-pass row merged into a repeat row credits first, a cf finding apply resolved gives unique 1, applied 1, scheduled 0, one only scheduled gives applied 0, scheduled 1, a refuted repeat row counts in findings only, and a same_as to U1 from U2 counts in cross_run only. It also checks the pooled sums per depth and that the store is left byte-identical.

The narrow nextest check is marked `Run by: gate`, so I did not run it. clippy `--all-targets` compiles the new test cleanly. The reference doc for the flag (`claude/skills/tomlctl/references/review.md`) and `measurement.md` belong to other tasks.
