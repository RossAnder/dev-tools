Status: done — review plan --propose admits cells in three passes (judgement and context-free cells, a sweeper pool, then repeats) and takes --context-free and --reserve

```toml run-note
files_touched = ["tomlctl/src/review/cells/propose.rs", "tomlctl/src/review/cells/args.rs", "tomlctl/tests/review_propose_admission.rs", "tomlctl/tests/review_cells.rs"]

[[deviations]]
what = "A judgement-lens cut shrinks the sweeper pool to the held-back cells only when the cut lens is accountable (matched). A candidate whose signals did not hit can still be cut, and it still sets the stopped latch, but it leaves the pool alone"
why = "On a large scope, an unmatched per-file candidate such as doc-drift is nearly always cut. Read literally, the rule would then collapse the pool to one 400-file sweeper cell on almost every run, and the room freed would go to no judgement lens, since that pass is already over. Alternatives: the literal rule (simpler, but the pool would almost never be used), or ignoring cuts in the pool size altogether (against the spec)"
means = "detail: when J cut any judgement lens for `ceiling`, the pool is only the held-back cells"

[[deviations]]
what = "Context-free cells merge adjacent pairs first and then any pairs, the same judgement ladder the floor fit uses. They are capped at min(max(1, ceil(ceiling/6)), the room left in the judgement pass after the check cells). When even that cannot be reached, none are emitted and summary.context_free reads 0"
why = "Adjacent merges alone can stop above the cap when slices share no parent. The floor fit keeps one cell back for them whenever --context-free is passed, so emitting none happens only at a tiny budget"
means = "detail: merged adjacent down to at most max(1, ceil(ceiling/6)) cells"

[[deviations]]
what = "The floor fit's soft target is now half the judgement-pass room (budget minus held-back cells) counted in first-pass cells. Its hard limit keeps back room for the check cells plus one context-free cell"
why = "The detail says measure counts first-pass cells only but names no new soft target. Half of the first-pass room keeps the old 'half the budget' shape, and repeats now fill only what is left over"
means = "FloorFit with measure counting first-pass cells only"

[[deviations]]
what = "Cells from the judgement pass are listed first, then sweeper cells, then repeats, so sweeper rows now follow the check and later judgement rows in the NDJSON"
why = "The sweepers are admitted in their own pass after the judgement lenses. No consumer depends on row order: the append path is keyed by content"
means = "today's cell order"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/review/cells/propose.rs tomlctl/src/review/cells/args.rs tomlctl/tests/review_propose_admission.rs tomlctl/tests/review_cells.rs"
result = "pass"

[[checks]]
command = "grep -q 'long = \"context-free\"' tomlctl/src/review/cells/args.rs && grep -q 'long = \"reserve\"' tomlctl/src/review/cells/args.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^review_propose_admission::/) | test(/^review_cells::/) | test(/^review_propose_slices::/) | test(/^review_ceiling::/)'"
result = "pass"
```

## What changed

- `cells/args.rs`: adds `--context-free` (bool) and `--reserve <N>` (usize, default 0). Both conflict with `--ndjson`.
- `cells/propose.rs`: `propose` now runs three passes. The budget is `ceiling - planned - reserve`.
  - **Picks are classified first.** Removed, after-wave and executor picks are handled as before. Every other pick is sliced and bound, then sorted into judgement lenses or sweepers.
  - **Judgement pass.** Each floor sweeper holds back one cell, and the rest of the budget is the room here. Counting is first-pass cells only.
    - Floor lenses use `FloorFit`, now first-pass only. The whole-scope collapse needs every scope file to be of a kind the lens reads (`single_kind_scope`).
    - Right after the floor lenses, `add_extra_cells` adds the `review-finder-cf` cells under `--context-free`, built from the code-kind slices `correctness` was admitted with, then the check cells. `--context-free` is refused at quick.
    - Other lenses keep fit-or-cut and the `stopped` latch. A signal-matched lens (signal tier, or a candidate whose signals hit) is first merged down by the judgement ladder, and is cut only when it still does not fit.
  - **Sweeper pool.** The pool is `min(max(1, ceil(ceiling/4)), room)`, or the held-back cells when a matched judgement lens was cut.
    - Slicing starts from `split_to_cap`, then runs one merge ladder: capped adjacent, capped any, uncapped adjacent, uncapped any.
    - Each floor sweeper still to come keeps one cell of the pool, and a floor sweeper falls back to one whole-scope cell.
  - **Repeats** fill the room left, judgement cells only, with hotspot cells first, then floor cells, then signal cells. No repeat is proposed while a signal-matched lens is listed as cut for `ceiling`.
  - **Summary** gains `reserve`, `mechanical_pool` and `context_free`. `over_cap` is computed as before, so it now lists the sweeper cells the pool merged past 40 files.
  - Context-free rows carry the question and reason the carrier used to write.
- `tests/review_propose_admission.rs`: six tests.
  - A 400-file standard scope keeps sweepers within a pool of 4, never repeats a sweeper, keeps security while repeats exist, and repeats hotspot cells first.
  - `--reserve 2` gives 14 cells where the full proposal gives 16.
  - `--context-free` cells number at most 3, cover correctness's slices and append cleanly. The flag is refused at quick.
  - A per-file signal lens merges down to fit.
  - A code-plus-prose signal lens that cannot fit in one cell is cut, nothing repeats, and each sweeper holds exactly one cell.
- `tests/review_cells.rs`: changed tests.
  - `propose_respects_scaled_ceiling`: doc-drift, a candidate whose signal hit, now merges into the one cell left instead of being cut.
  - `propose_after_a_planned_wave_lists_only_uncovered_concerns_as_ceiling_cuts`: the planned wave leaves doc-drift out, so the second proposal still lists it as cut.
  - `propose_adds_reverse_order_repeat_cells_at_standard`: sweeper cells no longer repeat, and hotspot repeats come first.
  - `propose_merges_small_floor_slices_before_cutting_signal_lenses`: the floor lens's first pass is at most 7, half of the 15 cells left beside ripple's held cell.
  - `propose_caps_sweeper_slice_by_file_count`: now at standard, where the pool of 4 holds the three 40-file chunks.
  - `propose_reports_over_cap_cells_at_a_small_ceiling` passes unchanged.

Not covered here: the carrier and `compose.md` still append their own context-free rows and pass no `--reserve`. Those files belong to other tasks. The `review.md` flag table also needs the two new flags, and it is held by a sibling task.
