Verdict: `propose` (tomlctl/src/review/cells.rs:1043) is pick, slice, shape, bundle and repeat in one loop. Every requested change lands in cells.rs. Slicing is keyed only by stack root or top-level dir (cells.rs:603-613). Security inherits correctness's slices through the `shaped` cache and bundles onto its cells at no cost (cells.rs:1124, 1144). Sweepers split at a fixed 40 files (cells.rs:690). Repeats are counted in every admission test (cells.rs:773, 1143). Eleven tests in tomlctl/tests/review_cells.rs pin this today.

```toml sources
read_at = "00ff2113dbfab0b7db0b98df71442194340b81c6"
dirty = []

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 1043
claim = "propose: the budget, the pick loop, floor fit, the non-floor fit test, repeats taken from the room left, the bind check and the summary"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 433
claim = "pick_lenses tiers Floor<Forced<Signal<AlwaysAt<Candidate; candidates sort by signal hits; accountable = tier != Candidate || hits > 0"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 588
claim = "slices_for: whole-scope unless per-subsystem/per-file; per-subsystem key = longest stack root else top-level dir; each hotspot is its own slice; no file-kind or signal-root input"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 280
claim = "Signals::of keeps roots only for facet binding (with_roots) and stack roots; keeps evidence only for churn-hotspot"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 690
claim = "split_to_cap chunks any mechanical lens's slice at SWEEPER_SLICE_CAP = 40 files, per lens, with no limit on the number of cells"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 768
claim = "FloorFit: the soft target is budget/2 counted with repeats; the hard limit is budget minus the check reserve; soft merges keep the 40-file cap"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 851
claim = "bundle joins a cell of the same mode with an identical slice while concerns < per_cell_max (3, or 2 at deep)"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 885
claim = "repeats: standard repeats floor or hotspot cells, deep repeats hotspot or signal cells, each with its slice reversed and pass 2"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 1124
claim = "the shaped cache, keyed by (sweeper, raw slices), makes a later lens with the same raw slices reuse an earlier lens's merged slices"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 1141
claim = "non-floor fit: joining existing bundles always fits; a new cell fits only if cells + repeats <= budget and nothing was cut earlier (stopped)"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 366
claim = "coverage_accounting reuses pick_lenses and its accountable flag"

[[source]]
path = "tomlctl/src/review/catalogue.rs"
line = 76
claim = "SliceRule has four values: whole-scope, per-subsystem, per-file, per-task"

[[source]]
path = "tomlctl/src/review/catalogue.rs"
line = 759
claim = "Catalogue::routing() exposes the merged [routing] executable and tests globs (used by add.rs, triage.rs, select.rs)"

[[source]]
path = "tomlctl/src/review/runs.rs"
line = 717
claim = "a judged signal is written with optional roots and evidence; the surveyor gives 1-5 evidence paths"

[[source]]
path = "claude/skills/review-v2-lenses/lenses/security.md"
claim = "security: judgement, signal tier on trust-boundary/dependency-change/build-script-change/package-lifecycle-change, slice per-subsystem"

[[source]]
path = "claude/skills/review-v2-lenses/lenses/test-gaps.md"
claim = "test-gaps: judgement, candidate tier on finding-hotspot, slice per-subsystem"

[[source]]
path = "claude/skills/review-v2-lenses/lenses/ripple.md"
claim = "ripple: mechanical, floor, slice whole-scope, so it is split at 40 files over the whole scope"

[[source]]
path = "claude/skills/review-v2-lenses/vocabulary.toml"
line = 703
claim = "the [routing] executable and tests globs; dependency-change and docs-touched detect globs name lockfiles and prose"

[[source]]
path = "claude/skills/review-v2-engine/SKILL.md"
line = 99
claim = "a run's scope is normally the changed files, so ScopeView.files is the diff's files; the run stores no separate changed-file list"

[[source]]
path = "tomlctl/tests/review_cells.rs"
line = 472
claim = "the integration tests pinning propose's current behaviour (lines 472-1100)"

[[source]]
path = "claude/skills/tomlctl/references/review.md"
line = 208
claim = "documents floor merge to half the budget counted after repeats, the 40-file sweeper split and over_cap"

[[source]]
path = "claude/skills/review-v2-engine/references/compose.md"
line = 36
claim = "documents the proposal order, bundling, repeats and the 40-file sweeper split; line 49 tells the orchestrator to re-split by subsystem"
```

## The pipeline as it runs today

1. **Budget** (cells.rs:1063-1070). `ceiling` (cells.rs:144) is `min(depth cap 6/16/32, max(3, ceil(changed_lines/100)))`. The budget is that ceiling minus the run's non-executor cells already planned. `per_cell_max` is 3, or 2 at deep (cells.rs:129). `reserve = ceil(checks/3)` (cells.rs:1082).
2. **Pick** (`pick_lenses`, cells.rs:433-489). The tier order is Floor < Forced < Signal < AlwaysAt < Candidate. Candidates sort by signal hits, highest first, then by id. A candidate whose signal matched (test-gaps on `finding-hotspot`, doc-drift on `docs-touched`, hygiene) stays in tier Candidate. Only its `accountable` flag marks it as matched (cells.rs:477). `coverage_accounting` reuses this function (cells.rs:378).
3. **Checks** are inserted before the first non-floor lens as whole-scope finder cells, 3 checks per cell (cells.rs:1093, 1295).
4. **Slice** (`slices_for`, cells.rs:588-626). Only `per-subsystem` and `per-file` slice; every other rule gets the whole scope. Under per-subsystem, each file is keyed by the longest stack root that holds it, else its top-level directory. Each churn hotspot becomes a slice of its own. `compact_tree` (cells.rs:555) turns a group into `dir/**` only when the group holds every tracked file under its common directory. Slicing never looks at a file's kind (code, tests, prose, lockfile) or at any signal's roots or evidence. `Signals::of` (cells.rs:280-327) does not even keep a judged signal's evidence. It keeps roots only in `with_roots`, for facet binding.
5. **Shape.** A mechanical lens goes through `split_to_cap` at 40 files (cells.rs:690). That cap applies per lens, with no limit on the number of cells. A floor lens is fitted by `FloorFit` (cells.rs:768-811): it merges toward `soft = budget/2`, counted *with repeats* (cells.rs:773, 797), with the 40-file cap kept on sweepers. Past that it merges toward `hard = budget - reserve`, dropping the cap. A non-floor lens is admitted when it joins existing bundles only (`trial.len() == cells.len()`), or when `cells + repeats <= budget` and no earlier lens was cut (cells.rs:1141-1145). The `shaped` cache, keyed by `(is_sweeper, raw slices)` (cells.rs:1087, 1124), hands a later lens with the same raw slices the earlier lens's *merged* slices.
6. **Bundle** (cells.rs:851). A lens joins a cell of the same mode with an identical slice while that cell has room under `per_cell_max`.
7. **Repeat** (cells.rs:885). At standard, every floor cell and every cell holding a hotspot repeats. At deep, every hotspot cell and every signal cell repeats. Each repeat reverses the slice and sets pass 2. Repeats are generated after the loop and limited to the room left (cells.rs:1171-1175). They are also reserved in advance by both fit tests above.
8. **Executor cells** for plan-executor sit outside the ceiling (cells.rs:1177-1206). Every cell is bind-checked (cells.rs:1213). The summary carries `over_cap` for sweeper cells over 40 files (cells.rs:1237).

## Findings

- **Library/API or file**: tomlctl/src/review/cells.rs:603 `slices_for` (PerSubsystem key)
- **Source**: cells.rs:603-613 read; vocabulary.toml:703 `[routing]`; catalogue.rs:759 `Catalogue::routing()`
- **Evidence-grade**: high
- **Finding**: Per-subsystem slices group by stack root or top-level directory only. `Cargo.lock` at the root falls in the `.` group, and agent prose and test files share a group with the code beside them. Subsystem slices that keep lockfiles and prose apart from code land here, as a file-kind split of the group key.
- **Details**: `routing().executable` and `routing().tests` already classify paths, and add.rs, triage.rs and select.rs already compile them. Keying a group by (subsystem, kind) is the minimal change. `compact_tree` (cells.rs:555) then stops producing `dir/**` for mixed directories and emits explicit file lists. That is safe, because it compacts only when the group holds the whole tree, but slices get longer. The test fixture vocabulary (tests/review_cells.rs:15-81) has no `[routing]` table, so kind classification needs a fallback or fixture globs. The plan must decide whether judgement code lenses drop lockfile and prose slices or still get them. ripple is whole-scope, so those files stay sliced either way and `unsliced` and `coverage_accounting` are unaffected (cells.rs:404, 1236).
- **Counter**: Refuted if the engine's scope never holds lockfiles or prose. It does: run U3's proposal mixed Cargo.lock, prose and tests (backlog B-abc59fb3).
- **Impact on plan / report**: The change is in cells.rs `slices_for` and `ScopeView`. It needs a catalogue routing read in `propose`, a decision on where non-code kinds go, and new fixture routing in the test file.

- **Library/API or file**: tomlctl/src/review/cells.rs:1124 `shaped` cache and cells.rs:1144 non-floor fit
- **Source**: cells.rs:1087, 1124-1128, 1141-1145; lenses/security.md and test-gaps.md front matter (`slice = "per-subsystem"`)
- **Evidence-grade**: high
- **Finding**: security and test-gaps use the same rule as correctness, so they get the same raw slices and the same cache key. Each therefore reuses correctness's merged floor slices and bundles into correctness's cells, up to `per_cell_max` 3. A join that adds no cell always passes the fit test. This is the mechanism behind the U3 bundle of correctness, security and test-gaps.
- **Details**: Giving security or test-gaps its own slices changes its cache key. It then needs new cells and goes through the `cells + repeats <= budget` test with the `stopped` latch (cells.rs:1153). Today a security cut is impossible while a bundle has room. After the change, security can be cut for `ceiling`. The repeat-ordering change below is what protects it.
- **Counter**: Refuted if `bundle` compared slices by file set rather than by entries; it compares `c.slice == slice` (cells.rs:856).
- **Impact on plan / report**: The security and test-gaps changes, and the repeat-ordering change, must land together or in that order.

- **Library/API or file**: tomlctl/src/review/cells.rs:280 `Signals::of` and cells.rs:588 `slices_for` signature
- **Source**: cells.rs:296, 310-313; runs.rs:717-743 (`parse_signal` stores `roots` and `evidence`); survey.md:36 (the surveyor writes 1-5 evidence paths and no roots); store.rs:1610 fixture (`trust-boundary` with `evidence` only)
- **Evidence-grade**: high
- **Finding**: Slicing security to the trust-boundary roots needs per-signal roots *and evidence* threaded into slicing. A judged trust-boundary normally carries evidence files, not roots, and `Signals::of` throws the evidence away. `slices_for` takes only the rule, so it cannot see which signals a lens matched.
- **Details**: There are two landing options. One is a new `SliceRule` variant, for example `per-signal-root`, in catalogue.rs:76-81, set in `lenses/security.md` with a row in the review-v2-lenses SKILL.md:48 table. The other special-cases this inside `slices_for`. Either way, `slices_for` must receive the lens's `apply_when.signals` and the union of the matched signals' roots, falling back to evidence. security also activates on `dependency-change`, whose roots are lockfile directories, so the union can legitimately include a lockfile directory. tests/review_cells.rs:915 and :960 accept trust-boundary with neither roots nor evidence, so a fallback to per-subsystem is needed to keep them green.
- **Counter**: Refuted if surveyors write roots for judged signals. survey.md:36 says they write evidence paths.
- **Impact on plan / report**: Touches cells.rs, catalogue.rs, lenses/security.md and the lenses SKILL.md slice table if a rule is added, plus a new test.

- **Library/API or file**: tomlctl/src/review/cells.rs:238 `ScopeView::new`
- **Source**: cells.rs:238-255; review-v2-engine/SKILL.md:99-105 (scope is the diff's files); store.rs:106 `Run` (no changed-file field)
- **Evidence-grade**: medium (assumes the engine's usual scope choice)
- **Finding**: To slice test-gaps over changed code plus its tests, `ScopeView.files` (the scope's tracked files, normally the changed files) supplies the code half. The tests have to come from `ScopeView.tracked` filtered by `routing().tests` within the same subsystem key, because unchanged tests are not in scope.
- **Details**: The catalogue has no test-gaps lens fixture (tests/review_cells.rs:129-243), so a new fixture lens and test are needed. Adding tracked tests outside the scope to a slice makes them part of `sliced`, which is harmless for `unsliced` (it filters scope files). `prepare_cell` path checks (cells.rs:1368) accept any repo-relative path.
- **Counter**: Refuted if test-to-code pairing needs more than subsystem co-location, for example Rust `#[cfg(test)]` modules inside the code file; those are already covered by the code slice.
- **Impact on plan / report**: Either a slice rule for test-gaps or a lens-specific path in `slices_for`. Shares the routing plumbing with the first finding.

- **Library/API or file**: tomlctl/src/review/cells.rs:690 `split_to_cap` / cells.rs:631 `SWEEPER_SLICE_CAP`
- **Source**: cells.rs:690-707, 787-794, 1237-1245; lenses/ripple.md (`activation = "floor"`, `slice = "whole-scope"`); B-4e15b9fa (422 files gave ceil(422/40) = 11 ripple cells of 16)
- **Evidence-grade**: high
- **Finding**: The cap on mechanical cells has no home today. `split_to_cap` chunks each mechanical lens at 40 files independently. FloorFit's soft rules keep that cap, so a floor sweeper keeps ceil(files/40) cells as long as the hard limit holds. Capping mechanical cells at about a quarter of the ceiling needs a cell quota, `max(1, ceiling/4)`, counted across all mechanical lenses. hygiene is a per-subsystem mechanical candidate and makes its own sweeper cells.
- **Details**: The likely shape: chunk size becomes `max(40, ceil(files / quota))` in `split_to_cap`, the soft MergeRule cap in `FloorFit::fit` (cells.rs:791) follows it, and the non-floor fit test refuses a mechanical lens that would push mechanical cells past the quota. `over_cap` (cells.rs:1242) and its docs (review.md:208, compose.md:36, 49) define "over cap" as more than 40 files, and need a new meaning.
- **Counter**: Refuted if the quota were already enforced in `FloorFit` through `hard`. `hard` is the whole budget minus the check reserve and treats sweepers no differently.
- **Impact on plan / report**: Breaks `propose_caps_sweeper_slice_by_file_count` (tests/review_cells.rs:1036). That test expects 3 ripple cells at a quick ceiling of 6, and a quarter of 6 allows 1 or 2. It may also change `propose_reports_over_cap_cells_at_a_small_ceiling` (:1071).

- **Library/API or file**: tomlctl/src/review/cells.rs:1143 non-floor fit and cells.rs:773 `FloorFit::measure`
- **Source**: cells.rs:770-775, 796-798, 1141-1145, 1171-1175; B-abc59fb3 (7 of 14 cells were repeats while hygiene and doc-drift were cut)
- **Evidence-grade**: high
- **Finding**: Repeats are reserved before later lenses in two places. The non-floor test admits a new cell only if `trial.len() + repeats(trial).len() <= budget`. Floor merging aims at `budget/2` counted with repeats. To admit repeats only after every signal-matched lens, drop repeats from the non-floor test for picks that are `accountable`, and keep the existing `take(room)` at cells.rs:1172 as the only place repeats get room.
- **Details**: "Signal-matched" spans `Tier::Signal` and `Tier::Candidate` with `hits > 0`, which is the `accountable` flag. Forced and AlwaysAt picks need an explicit ruling. When room is short, `take(room)` keeps repeats in cell order. That puts floor cells first, then signal cells, which may not be the order the plan wants. Whether floor `soft` keeps counting repeats is a separate decision.
- **Counter**: Refuted if repeats were generated before picks; they are generated after the loop (cells.rs:1172) and only reserved during it.
- **Impact on plan / report**: Pinned by `propose_merges_small_floor_slices_before_cutting_signal_lenses` (:960), whose "correctness.len()*2 <= 8" encodes the soft-with-repeats target, and by `propose_adds_reverse_order_repeat_cells_at_standard` (:747), which expects one repeat per due cell.

## Tests that pin today's behaviour (tomlctl/tests/review_cells.rs; cells.rs has no unit tests)

- `propose_includes_floor_and_matched_signal_lenses` (:473). Pins correctness's slice as `["src/**"]` in a fixture with a trust-boundary signal whose evidence is src/a.rs. A security slice built from the evidence would make it a separate cell.
- `propose_respects_scaled_ceiling` (:523). Pins the ceiling formula and doc-drift's per-file `src/x/*` slices.
- `propose_after_a_planned_wave_lists_only_uncovered_concerns_as_ceiling_cuts` (:593).
- `propose_bundles_same_mode_concerns_only` (:710). Pins correctness+idioms and ripple+hygiene bundles, which kind or quota splits could change.
- `propose_adds_reverse_order_repeat_cells_at_standard` (:747).
- `propose_never_exceeds_ceiling` (:915).
- `propose_merges_small_floor_slices_before_cutting_signal_lenses` (:960).
- `propose_counts_check_cells_against_ceiling` (:999).
- `propose_caps_sweeper_slice_by_file_count` (:1036).
- `propose_reports_over_cap_cells_at_a_small_ceiling` (:1071).
- `accounting_lists_unselected_and_unsliced_until_waived` (:1227), through `pick_lenses`.

Coverage: read in full tomlctl/src/review/cells.rs, and tomlctl/tests/review_cells.rs lines 1-1100. Read the front matter of the security, test-gaps, correctness, ripple, hygiene, doc-drift and duplication lenses; the vocabulary.toml signals and routing; runs.rs `parse_signal` and `upsert_signals`; catalogue.rs `SliceRule`, `effective_activation`, `is_mechanical_lens` and `Routing`; the store.rs `Run` and `Signal` structs; compose.md:36-83; review.md:206-208; engine SKILL.md:99-105; backlog B-abc59fb3, B-4e15b9fa and B-f00449a1. Checks run: rg for callers of `pick_lenses`, `slices_for`, `Signals::of` and `SWEEPER_SLICE_CAP` (cells.rs only) and for `--propose` tests (review_cells.rs only); git status on the cited paths (clean). Ruled out: `compact_tree` absorbing split-off lockfiles back into `dir/**`, which it cannot, since it compacts only when the group holds every tracked file under the directory. Left to the sibling lens: enforcing the ceiling on append (cells.rs:1445 `append` computes no ceiling, B-f00449a1).
