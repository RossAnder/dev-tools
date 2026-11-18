Verdict: restructure: `review plan --propose` admits cells in phases (judgement lenses and checks, then sweepers in a quarter-of-ceiling pool merged past the 40-file cap, then repeats from leftover room) over slices that keep file kinds apart and come from per-lens slice rules. The ceiling refusal, the triage escalation path, schedule ordering and stats extend their modules in place.

```toml sources
read_at = "00ff2113dbfab0b7db0b98df71442194340b81c6"
dirty = []

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 481
claim = "pick_lenses sorts by tier, then hits, then id. Floor ripple (mechanical) comes before every signal-tier judgement lens, and hygiene (a candidate with hits) comes before zero-hit judgement candidates"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 1092
claim = "propose is one greedy loop in tier order. Each lens's cells are committed by bundle at 1149, and nothing later re-merges a committed lens's slices"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 796
claim = "FloorFit::fit merges only the current pick's slices. Its soft target counts repeats (measure at 770-775)"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 1143
claim = "Non-floor admission counts repeats in `total`. A matched lens can be cut for `ceiling` while repeats are later kept by take(room) at 1171-1175"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 1090
claim = "The `stopped` latch: once one lens is cut, later lenses only bundle"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 588
claim = "slices_for takes only the SliceRule and Signals. Per-subsystem grouping is by stack root or top directory, whatever the file kind"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 270
claim = "Signals keeps per-signal evidence only for churn-hotspot (310-313). Trust-boundary evidence is dropped"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 555
claim = "compact_tree emits dir/** whenever every tracked file under the common dir is in the group, so a lockfile under a stack root rides along"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 715
claim = "merge_step joins consecutive slices, including non-adjacent ones under rules (false, cap) and (false, None) at 792-794, with no notion of file kind"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 802
claim = "FloorFit collapses a single merged slice to the whole scope, which then holds every kind"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 690
claim = "split_to_cap splits sweeper slices at 40 files before fitting. Non-floor sweepers get no merge step"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 1445
claim = "append mints a wave with no ceiling check. ceiling() at 144 and the non-executor planned count at 1064-1068 live only in propose"

[[source]]
path = "tomlctl/src/review/catalogue.rs"
line = 76
claim = "SliceRule has four values. The lens frontmatter `slice` is parsed through it (1942), so a new rule is a new variant plus frontmatter"

[[source]]
path = "tomlctl/src/review/catalogue.rs"
line = 2123
claim = "The [routing] parser accepts only `executable` and `tests` keys"

[[source]]
path = "tomlctl/src/review/fields.rs"
line = 1363
claim = "RoutingGlobs holds the executable and tests globs as private fields: the existing code/test classifier"

[[source]]
path = "claude/skills/review-v2-lenses/vocabulary.toml"
line = 232
claim = "The trust-boundary hint says its evidence is the file holding the entry point"

[[source]]
path = "claude/skills/review-v2-lenses/vocabulary.toml"
line = 83
claim = "dependency-change detect globs list the lockfiles"

[[source]]
path = "claude/skills/review-v2-lenses/lenses/security.md"
line = 10
claim = "security is per-subsystem today, over four signals"

[[source]]
path = "claude/skills/review-v2-lenses/lenses/test-gaps.md"
line = 9
claim = "test-gaps is per-subsystem today"

[[source]]
path = "claude/skills/review-v2-lenses/lenses/ripple.md"
line = 5
claim = "ripple is a mechanical, floor, whole-scope lens, so a 422-file scope splits into about 11 sweeper cells"

[[source]]
path = "claude/skills/review-v2-engine/SKILL.md"
line = 100
claim = "A diff run's scope is the files the change touches, so 'changed code' is scope files the routing classifier calls code"

[[source]]
path = "tomlctl/src/review/triage.rs"
line = 38
claim = "triage's positional ids are required. Every mode disposes stored findings by id"

[[source]]
path = "tomlctl/src/review/triage.rs"
line = 104
claim = "Each new mode flag must be added to the NOT_DEFER, NOT_CONVERT and NOT_ADDRESSED conflict lists"

[[source]]
path = "tomlctl/src/review/triage.rs"
line = 969
claim = "replacement_row and finish_row (1048) default and stamp from an advisory finding: lens, file, symbol, cell, related, run. The replacement is left unverified (1074)"

[[source]]
path = "tomlctl/src/review/add.rs"
line = 403
claim = "field_refusals refuses a lens outside the given cell's concerns unless the row is a cleanup with old under a swept lens"

[[source]]
path = "tomlctl/src/review/schedule.rs"
line = 410
claim = "blocked() needs status Open, so an evidence-blocked finding must be open to get a settle task"

[[source]]
path = "tomlctl/src/review/schedule.rs"
line = 578
claim = "Task needs come only from finding depends_on"

[[source]]
path = "tomlctl/src/review/schedule.rs"
line = 730
claim = "Fix-row constraints are composed in Compose::row"

[[source]]
path = "tomlctl/src/review/fields.rs"
line = 1439
claim = "Cleanup routing reads file, target_file and instances, not fix_files"

[[source]]
path = "tomlctl/src/review/verdict.rs"
line = 542
claim = "checker_may_refute is true for any finding whose cell is a sweeper's. A converted row inherits the advisory's cell"

[[source]]
path = "tomlctl/src/review/stats.rs"
line = 439
claim = "Tally counts audited and overturned in add_finding"

[[source]]
path = "tomlctl/src/review/stats.rs"
line = 571
claim = "by_model's inline ModelTally counts audited and overturned a second time, with its own JSON at 586-588"

[[source]]
path = "tomlctl/src/review/stats.rs"
line = 376
claim = "overturn returns a bool and an optional verdict, with no class"

[[source]]
path = "tomlctl/tests/review_cells.rs"
line = 270
claim = "The test Repo sandbox tracks any file list and seeds run rows with signals, so U3/U5 replays fit the existing harness"
```

## The drafted structure

### 1. Propose admission in phases (`cells.rs`, `propose`)

The claim: the single tier-ordered greedy loop will force the planner to backtrack to meet "sweepers merged past the 40-file split before any judgement lens is cut".

- `pick_lenses` sorts by tier, then id (cells.rs:481-487). Floor `ripple`, a mechanical lens, is admitted in the floor tier, so it lands before every signal-tier judgement lens, `security` included.
- Its cells are committed through `bundle` at cells.rs:1149.
- `FloorFit::fit` only merges the slices of the pick in hand (cells.rs:796-801), and no later step re-merges a committed lens.
- The same holds for `hygiene`. It is a candidate with hits, so it sorts ahead of zero-hit judgement candidates such as `test-gaps`, `idioms` and `type-design`. If it fits, it takes room first (cells.rs:1141-1145).
- A worked case at standard depth with a ceiling of 16: correctness takes about 8 cells (its soft target is budget/2), ripple stays at about 4 even when capped to a quarter, and the checks take 1 or 2. Security then fails and is cut, although ripple could have merged to 1 cell.

**Refuted if** a replay of U5 under the current loop, with only a quarter cap added at the `FloorFit` construction (cells.rs:1130-1138), never cuts a signal-tier judgement lens while ripple holds more than one cell.

There are three admission paths: `FloorFit` (soft target with repeats, at cells.rs:797), the inline non-floor check (cells.rs:1143, `total` with repeats) and `add_check_cells` (cells.rs:1295). Each has its own idea of what fits, and the `stopped` latch sits across them (cells.rs:1090). Each of the four new rules (quarter pool, admission on first-pass cells only, sweeper merging before a judgement cut, no cut while a repeat is kept) would have to be written into all three paths.

**Drafted shape.** It stays in one file and reuses `merge_step`, `shrink`, `bundle` and `repeats` unchanged.

- **Phase J.** Admit judgement lenses and the check cells in tier order. Count first-pass cells only. The ceiling is `budget − reserve(checks) − one cell per floor sweeper`.
  - Floor judgement lenses keep `FloorFit`, with `measure` counting first-pass cells only.
  - Other lenses keep greedy fit-or-cut, with the `stopped` latch scoped to this phase.
- **Phase M.** Admit mechanical lenses into `min(ceil(ceiling/4), room left)`.
  - Start from `split_to_cap`, then apply one merge ladder for every tier: capped adjacent, capped any, then uncapped.
  - A floor sweeper can always fall back to one whole-scope cell.
  - A non-floor sweeper is cut only once the uncapped merges cannot fit it.
- **Phase R.** Repeats `take(room)` as now (cells.rs:1171-1175). Since no admission counted them, no matched lens is ever cut for `ceiling` while a repeat is kept.
- The `unselected`, `covered` and summary code after the loop stays as it is.

### 2. Slices carry a file kind, and slice rules can be per lens

The claim: `slices_for` (cells.rs:588) and the merge rules will force kinds to mix on this change.

- Per-subsystem grouping keys on stack root or top directory (cells.rs:604-613). `compact_tree` then emits `dir/**` when every tracked file under the directory is in scope (cells.rs:555-568). This repo has `lumina/Cargo.lock` and `lumina/web/bun.lock` under a stack root, and the root `Cargo.lock` groups into `.` beside `Cargo.toml`.
- `merge_step` joins non-adjacent slices with no kind check (cells.rs:715-738, rules at 792-794).
- `FloorFit` collapses a single merged slice to the whole scope (cells.rs:802-808).

So a correctness cell holding a lockfile or agent prose beside code comes from three places: grouping, merging and the collapse. Fixing grouping alone leaves the other two.

**Drafted change.**

- `Slice` (cells.rs:634) gets a `kind` field: code, test, lockfile, prose or other.
- `merge_step` refuses cross-kind pairs for judgement lenses.
- The whole-scope collapse applies only when the scope is a single kind.
- `compact_tree` checks "all tracked files under the dir" against same-kind files only, so `dir/**` is never emitted where a different kind lives under the directory. `PathSet` has no negation (cells.rs:189-224), so the slice falls back to a file list.

The classifier already exists:

- `RoutingGlobs`, with private `executable` and `tests` fields (fields.rs:1363), needs `pub(crate)` match methods.
- Lockfile and prose globs exist today only as signal detect globs (vocabulary.toml:83-88, 153-156, 56-59).
- The cleaner home is new `[routing]` keys. That forces `f.only(&["executable", "tests"], ...)` at catalogue.rs:2123 to widen, and `Routing` (catalogue.rs:511) to grow.

**Refuted if** the U3 replay's correctness cells contain no non-code file after only the grouping key changes.

Security and test-gaps need per-lens slicing inputs:

- `Signals` drops every signal's evidence except churn-hotspot's (cells.rs:310-313).
- The trust-boundary evidence is "the file holding the entry point" (vocabulary.toml:235).

**Drafted change:** two `SliceRule` variants (catalogue.rs:76), set in lens frontmatter (security.md:10, test-gaps.md:9). The frontmatter changes the lens's metadata, not its question text.

- `signal-evidence`: the evidence and roots of the lens's matched signals, falling back to `per-subsystem` when none carry files. This also sends dependency-change lockfiles to security, which is the one judgement lens where they belong.
- `changed-with-tests`: the code-kind scope files, which are the changed files on a diff run (SKILL.md:100-101), plus tracked test-kind files under the same subsystem root.

`slices_for` then takes the `Pick`, or the lens's signals, together with the classifier. The `shaped` cache key at cells.rs:1124 still works because it keys on the slices themselves.

### 3. The pieces that extend in place

- **Ceiling refusal.** `append` (cells.rs:1445) has no ceiling check. It needs `ceiling(run)` (cells.rs:144) and propose's non-executor count (cells.rs:1064-1068), which should be lifted into one shared helper so the two cannot drift.
  - The override reason should be a staged file. It is written to the run's `rest`, in the same `mutate_minting` closure that mints the wave, and declared as a `Run` field in fields.rs:558-585 like `rechecks`.
  - It is then surfaced by `runs.rs` and `stats.rs` `by_run` (stats.rs:688-700).
  - `replayed_wave` (cells.rs:1487) must short-circuit before the check, so a replay of an overridden wave is not refused.
  - **Interaction:** at `quick`, the escalation probe cell counts against the ceiling (SKILL.md:231). Follow-up waves count too. Once the proposal fills the ceiling, both will hit the new refusal, so the skill prose must say when to pass the override.
- **Escalation to settle.** This can be a triage mode, but four things in the current code force edits:
  1. `ids` is required (triage.rs:38-46), and an escalation has no finding id. The escalating cell id could serve as the id.
  2. The new flag joins all three conflict lists (triage.rs:104-149).
  3. `replacement_row` and `finish_row` take defaults, the cell, `related` and the run from an advisory (triage.rs:969-1077). Generalise their source to an advisory finding or an escalation `{cell, text}`, rather than copying the function.
  4. `add::field_refusals` with the escalating cell refuses any lens outside that cell's concerns (add.rs:403-415), and an escalation is by definition outside them. Pass no cell, or set the lens to one of the cell's concerns.

  The minted row must be `open`, not `unverified` as convert leaves it (triage.rs:1074), because `schedule`'s `blocked()` needs Open (schedule.rs:410).
- **Multi-file cleanup.** `review schedule` already folds `fix_files` into one task (schedule.rs:449-458, 471). Cleanup routing ignores `fix_files` (fields.rs:1439). `checker_may_refute` is true for any finding whose cell is a sweeper's (verdict.rs:542-549), and a converted row inherits the advisory's cell (triage.rs:1069). This is a route and field change in `fields.rs`, `add.rs` and `verdict.rs`, not a module move.
- **Doc task needs code task.** `Compose::plan` builds `needs` only from `depends_on` (schedule.rs:578-597). `task_of` already maps findings to predicted task ids, so the new edge goes in that loop, and the constraint goes into `Compose::row` (schedule.rs:730).
- **Stats.** The audit split forces the same edit twice: in `Tally` (stats.rs:411-412, 439-441, 468-470) and in `by_model`'s inline `ModelTally` (stats.rs:548-549, 571-573, 586-588). `overturn` returns no class (stats.rs:376).
  - Extract one audit tally keyed by class (CONFIRMED defect, REFUTED defect, checked cleanup) and use it from both. This is a local extraction.
  - The repeat and context-free yield is a new section over `Cell.pass`, cells with `agent == finder-cf`, `same_as` and `apply_state` (stats.rs:819).

## Coverage

**Files read in full:**
- `tomlctl/src/review/cells.rs`
- `tomlctl/src/review/schedule.rs`

**Files read in part:**
- `tomlctl/src/review/triage.rs`, lines 1-575 and 765-1083
- `tomlctl/src/review/stats.rs`, lines 1-710 and the outline
- `tomlctl/src/review/catalogue.rs`, enums, `Entry`, routing parser
- `tomlctl/src/review/fields.rs`, finding fields, `RoutingGlobs`, `route_cleanup`
- `tomlctl/src/review/verdict.rs`, lines 490-600
- `tomlctl/src/review/settle.rs`, header and refusals
- `tomlctl/src/review/survey.rs`, header and `load_diff`
- `tomlctl/src/review/store.rs`, the `Run` struct
- `tomlctl/src/review/add.rs`, lines 395-445
- `claude/skills/review-v2-lenses/vocabulary.toml`, the signals and routing sections
- the frontmatter of every lens
- `tomlctl/tests/review_cells.rs`, the harness and two tests
- greps over the review-v2-engine skill prose

**Alternatives ruled out:**
- **Split `cells.rs` into a `cells/` directory** (propose, slicing, append). No edit is forced by the file boundary, and the tests drive the CLI, so the split is a preference and is not claimed.
- **Extract a pure proposal core so U3 and U5 replay from fixtures.** The git sandbox at review_cells.rs:270-296 already tracks arbitrary file lists and seeds run signals, so the replays fit the current IO path.
- **Mint escalations as store advisories at wave time, so `--convert` covers them.** This changes the wave and compose flow and the escalation payload files. Convert's replacement goes to `unverified` and its verifier route, not to settle.
- **Encode the budget rules in each existing admission path.** This is ruled out by the ordering claim in section 1.
- **Restructure triage, schedule or stats.** Each change lands in one function or one tally, with no edit forced across a module boundary.

No build or test was run. Whether the phased admission keeps `propose_never_exceeds_ceiling` and `propose_merges_small_floor_slices_before_cutting_signal_lenses` green is for the orchestrator's suite run.
