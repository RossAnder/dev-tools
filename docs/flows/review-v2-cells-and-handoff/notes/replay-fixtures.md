Verdict: Both proposals replay exactly without either repository: `propose` (tomlctl/src/review/cells.rs:1043) reads only the run record with no cells of its own, a git index of path names, the built-in catalogue and the `--lens +intent-fidelity` flag. A replay of both reproduced the saved proposal.ndjson exactly. The proposals themselves sit in git-ignored run dirs, so a fixture must carry its own copies.

## Answer

**Where the stores live.**
- U3 is run `U3` in `.claude/flows/mid-run-requests/review-v2.toml` (tracked, this repo). Its run dir is `.review-v2-runs/mid-run-requests/U3/`, holding `proposal.ndjson`, `pack.md`, `profile.md` and `report.md`.
- U5 is run `U5` in the reportdesignkit repo, at `.claude/flows/package-design-tools/review-v2.toml` (tracked there, last touched by its commit `edd1c26f`). Its run dir is `.review-v2-runs/package-design-tools/U5/`.
- Both run dirs are git-ignored (`.review-v2-runs/.gitignore` is `*`). So both `proposal.ndjson` files exist only on this machine.

**What `--propose` emitted, and what the orchestrator wrote.**
- U3 (code, standard, 3907 changed lines, ceiling 16, 46 scope files). The proposal had 14 cells:
  - 4 correctness+security+test-gaps cells sliced by the file rule. They mixed `Cargo.lock`, agent prose and test files.
  - 2 ripple sweepers, split 40 + 6.
  - 1 intent-fidelity+doc-consolidation+architecture cell over the whole scope.
  - 7 pass-2 repeats.
  - hygiene and doc-drift were cut for the ceiling.
- The orchestrator's U3 map was C18–C31:
  - correctness alone on the request core (C18) and on holders/lane/check (C19).
  - security only on the trust-boundary modules (C20).
  - test-gaps only on test files (C21).
  - correctness+doc-drift on harness prose (C22) and on the reference docs and manifests (C23).
  - the proposed ripple pair and whole-scope cell kept as proposed (C24, C25, C27).
  - hygiene added (C26).
  - two context-free correctness cells (C28, C29).
  - two repeats instead of seven (C30, C31).
- U5 (code, standard, 30619 changed lines, ceiling 16, 422 scope files). The proposal had 16 cells:
  - 4 correctness+domain-model+interaction cells of 102, 79, 74 and 167 slice entries.
  - 11 ripple sweepers at 40 entries each (the last one 22).
  - 1 pass-2 repeat of 102 entries.
  - 11 lenses cut for the ceiling: intent-fidelity, contrarian, doc-consolidation, external-validity, security, ui-visual, hygiene, type-design, architecture, doc-drift and seams.
- The orchestrator's U5 map was C46–C61, plus C62 in wave 2:
  - 6 judgement cells split by subsystem, each bundling the lens that a signal or subsystem pointed to: security on the sidecar, seams on core, architecture on shell, type-design on shared views.
  - 3 context-free correctness cells.
  - 3 ripple+hygiene sweepers over 71, 133 and 218 entries.
  - intent-fidelity (C58), contrarian (C59) and doc-drift (C60).
  - a 9-file hotspot repeat (C61).
  - a 17th probe cell, C62, past the ceiling.
- The context-free cells in both maps are carrier additions, never `--propose` output. compose.md:87 makes the carrier add them, and `--propose` never emits `review-finder-cf`. A comparison of proposal against map has to count them apart.

**What a replay fixture needs.** I checked this by replay: in my own scratch directory, outside any checkout, I ran `git init` over a tree of empty files plus a one-run store. Both replays matched the saved proposals exactly.
1. **The run record, stripped of cells and run outcomes.** The fields read are `id`, `family`, `depth`, `changed_lines`, `scope` and `signals`. For each signal it reads `name`, `source` and `accepted`, `roots` (for facets and stack roots), and `evidence`, but evidence only from the `churn-hotspot` detector, whose top decile become hotspots. It also reads `catalogue`. The store's own cells for the run must be absent: `budget = ceiling − planned`. Run against the live U3 store, `--propose` now emits 2 cells and puts its 40-file ripple sweeper in `over_cap`.
2. **The tracked-file list at the run's head, path names only.** Use `git ls-tree -r --name-only <head>`: 2366 paths for U3 at `c7c85ae9`, 3249 for U5 at `d1930996`. File contents are never read. The scope list alone is not enough: compaction into `dir/**` and `dir/*` checks every tracked sibling. A scope-only replay kept the same cells and concerns but changed slice entries in 6 of the 15 U3 rows and 4 of the 17 U5 rows. The current harness needs a real git index, because `list_tracked` shells out to `git ls-files` and has no injection seam.
3. **A catalogue root.** U3's record pins `tree_sha`/`config_sha` and a project layer, and U5's pins `tree_sha` only. Both shas equal today's catalogue, `381fad85…`, because the catalogue has been unchanged since `b684520a`. A pinned `tree_sha` makes the replay refuse as soon as the catalogue changes, which this plan will probably do. Recording `catalogue = { builtin = "<repo>/claude/skills/review-v2-lenses" }` with no shas avoids the check, and U3 still replays exactly without its project layer: `harness-gates` is not a cell check.
4. **Flags.** Both runs need `--lens +intent-fidelity`, which compose.md:33 adds on every flow-bound code run. Without it, U3 bundles `duplication` where the original has `intent-fidelity`. Neither run needs `--accept-signals`: trust-boundary (and new-seam on U5) is already `accepted = true` on the record.
5. **What is not needed.** No task store (no executor lens in the code family) and no diff or base checkout: `changed_lines` is already on the record.

`tomlctl/tests/review_cells.rs` already builds most of this. `Repo::new` writes the tracked files into a git sandbox, and `run_row` writes `catalogue = { builtin = … }` with no shas. What it lacks is a loader for a recorded run, its tracked list and the real built-in catalogue in place of `write_catalogue`'s synthetic one.

## Findings

- **Library/API or file**: tomlctl/src/review/cells.rs:1043 `propose`
- **Source**: scratch replay. Each run's record went into a fresh `git init` root with empty files at every `git ls-tree` path of its head, then `TOMLCTL_ROOT=<root> tomlctl review plan --scope replay --run <id> --propose --lens +intent-fidelity`. The result compared JSON-equal to the saved proposal: `15 15 True` for U3 and `17 17 True` for U5.
- **Evidence-grade**: high
- **Finding**: Both proposals replay exactly from the run record, the tracked path list, the catalogue root and one flag, with no checkout of either repository.
- **Details**: The inputs are read at cells.rs:1045 (`repo_or_cwd_root`, which `TOMLCTL_ROOT` overrides), 1046–1048 (store, run, `run_catalogue`), 1061 (`Signals::of`), 1062 (`ScopeView::new` over `list_tracked`) and 1064–1069 (cells already planned on the run). The task store at 1180 is read only for executor lenses, and the code family has none.
- **Counter**: The replay would not prove input sufficiency if a proposal depended on the working tree at proposal time rather than on the index at `head`. Both matched exactly, so for these two runs it did not.
- **Impact on plan / report**: A replay test can carry these two runs as golden fixtures: the record as TOML, the tracked list as text, `proposal.ndjson` as the baseline and the orchestrator's cell map as the target shape.

- **Library/API or file**: tomlctl/src/review/cells.rs:1064 (`planned` and `budget`)
- **Source**: `tomlctl review plan --slug mid-run-requests --run U3 --propose` against the live store. Its summary line read `"cells":2`, and `over_cap` held the 40-file ripple slice.
- **Evidence-grade**: high
- **Finding**: A fixture cannot be the real store as is. The run's existing cells shrink the budget to `ceiling − 14`, so the fixture must hold the run record without its cells.
- **Details**: `budget = ceiling.saturating_sub(planned)` counts every non-executor cell on the run. `covered` (1162) also drops ceiling cuts that a stored cell already carries. The replay record drops `finished`, `counts`, `audit`, `agents` and `waves` too. `propose` does not read them, but a finished run might confuse a later `--ndjson` step in the same test.
- **Counter**: If `propose` gained a flag to ignore stored cells, the live store could be used directly. No such flag exists in `ReviewPlanArgs` (cells.rs:41–79).
- **Impact on plan / report**: The fixture loader strips `[[cells]]` for the run, or the fixture keeps a cell-free copy of the record.

- **Library/API or file**: tomlctl/src/review/cells.rs:165 `list_tracked`, :555 `compact_tree`, :572 `compact_dir`
- **Source**: a scope-only replay, with only the scope files tracked. It came out `15 15 False` for U3 (rows 0–2 and 7–9 differ, e.g. `claude/agents/**` in place of file lists) and `17 17 False` for U5 (rows 0, 2, 3 and 15 differ).
- **Evidence-grade**: high
- **Finding**: A fixture must carry the full tracked path list at the run's head, not just the scope. The compaction step checks tracked siblings outside the scope.
- **Details**: `compact_tree` emits `dir/**` only when every tracked file under the group's common directory is in the group, and `compact_dir` does the same for direct children. `list_tracked` gets those paths from `git -C <root> ls-files` and has no other source. It returns an empty list when git fails, and every rule then falls back to the whole scope (cells.rs:590–593). So a fixture needs either a git index of empty files, as `review_cells.rs`'s `Repo::new` builds, or a new seam that feeds the list in. The lists are 2366 and 3249 paths, about 0.1–0.2 MB of text.
- **Counter**: The full list is not needed if the plan changes the slicing so compaction no longer reads tracked siblings, or if the fixture is pruned to the tracked files under the top-level directories that hold scope files. That pruning is untested here.
- **Impact on plan / report**: The fixture format includes `tracked.txt` per run. The test harness either commits nothing and runs `git add` over generated empty files, which the existing harness already does, or gains a `TOMLCTL_TRACKED`-style injection seam (not present today).

- **Library/API or file**: tomlctl/src/review/catalogue.rs:2316 `run_catalogue` drift check
- **Source**: `tomlctl review catalogue` printed `tree_sha 381fad85…` and `config_sha 43eed0e7…`, both equal to U3's record (U5 records only the same `tree_sha`). A U3 replay with `catalogue = { builtin = "<repo>/claude/skills/review-v2-lenses" }` and no shas or project layer also came out `True`.
- **Evidence-grade**: high
- **Finding**: Keeping the recorded `tree_sha` makes the fixture refuse to load once the catalogue changes. Use a `builtin` path with no shas. That pins only the scope, signals and tracked list, and lets catalogue edits from this plan flow into the replay.
- **Details**: A record with `tree_sha` and no `builtin` resolves the built-in root through `default_builtin_root()`, which is `CLAUDE_CONFIG_DIR/skills/review-v2-lenses`, and errors with "the lens catalogue changed since the run started" on any drift. A test must not depend on the installed config dir. U3's project layer (`.claude/review-lenses/config.toml`, one non-cell `harness-gates` check) is not needed for the proposal. For a byte-exact baseline frozen at proposal time, vendor the catalogue at `b684520a` instead, since `claude/skills/review-v2-lenses` has not changed since that commit.
- **Counter**: Byte equality with the saved `proposal.ndjson` holds only while the live catalogue keeps `tree_sha 381fad85…`. Once this plan edits a lens's `slice` or `activation`, a live-catalogue fixture should assert properties, not equality.
- **Impact on plan / report**: Decide per test whether the baseline is a frozen golden (vendored catalogue) or a property check against the live catalogue, such as "test-gaps slices hold only test files" or "no more than N sweeper cells".

- **Library/API or file**: claude/skills/review-v2-engine/references/compose.md:33 (and :87)
- **Source**: a U3 replay without the flag differs only in rows 6 and 13, where `["doc-consolidation","architecture","duplication"]` stands in for `["intent-fidelity","doc-consolidation","architecture"]`. With `--lens +intent-fidelity` it is exact.
- **Evidence-grade**: high
- **Finding**: The command line is part of the fixture. Both runs need `--lens +intent-fidelity`, and the context-free cells in the orchestrator maps are carrier additions, never `--propose` output.
- **Details**: compose.md:33 adds `+intent-fidelity` on every code run bound to a flow with `plan.toml`, and both runs are. `intent-fidelity` has `activation = "opt-in"`. compose.md:87 has the carrier add the context-free correctness cell, and `--propose` never assigns `review-finder-cf` (`agent_for`, cells.rs:841). The orchestrator's maps (U3 C28/C29, U5 C52–C54) should therefore not be scored as a disagreement with the proposal.
- **Counter**: The command line would not need recording if the saved `pack.md` or `profile.md` already carried it. I did not check those files for the command line.
- **Impact on plan / report**: Each fixture records `args` (here `["--lens", "+intent-fidelity"]`). A "proposal vs actual" comparison excludes `review-finder-cf` cells and the post-ceiling probe C62.

Out of lens, one line for a sibling: the judged `trust-boundary` signal carries its files in `evidence` with `roots = []` on both runs. `Signals::of` (cells.rs:286–314) reads `evidence` only for `churn-hotspot`, so `--propose` today has no input that could point security at those files.

Coverage: I read `tomlctl/src/review/cells.rs` lines 1–360, 426–660 and 841–916 in part and 1000–1290 in full, `catalogue.rs` 680–712 and 2238–2340, and `tests/review_cells.rs` 250–420. I read the six backlog items through `tomlctl backlog get`, and both run records and cell maps through `tomlctl review run show`/`run list` and `tomlctl get … runs|cells`. I ran five scratch replays: full tracked list for U3 and U5, scope-only for U3 and U5, and U3 with the repo built-in and no project layer. Ruled out:
- the task store (no executor lens in the code family);
- `--accept-signals` (judged signals already `accepted = true` on the records);
- file contents (only path names are read);
- the project layer (U3 replays without it).

```toml sources
read_at = "00ff2113dbfab0b7db0b98df71442194340b81c6"
dirty = []

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 1043
claim = "propose reads root, store, run, catalogue, signals, tracked list and planned cells"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 1064
claim = "budget is ceiling minus cells already planned on the run"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 165
claim = "list_tracked shells out to git ls-files at the root, empty on failure"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 286
claim = "Signals::of reads name, source, accepted, roots, and evidence only for churn-hotspot"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 555
claim = "compact_tree and compact_dir consult tracked siblings outside the scope"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 841
claim = "agent_for never yields review-finder-cf, so context-free cells are carrier additions"

[[source]]
path = "tomlctl/src/review/catalogue.rs"
line = 2316
claim = "a recorded tree_sha or config_sha refuses the run's catalogue on drift"

[[source]]
path = "tomlctl/src/review/catalogue.rs"
line = 2238
claim = "default built-in root is the Claude config dir's skills/review-v2-lenses"

[[source]]
path = "tomlctl/tests/review_cells.rs"
line = 271
claim = "existing harness builds a git sandbox of tracked files and records catalogue.builtin without shas"

[[source]]
path = "claude/skills/review-v2-engine/references/compose.md"
line = 33
claim = "--lens +intent-fidelity is added on every flow-bound code run"

[[source]]
path = "claude/skills/review-v2-engine/references/compose.md"
line = 87
claim = "the carrier adds the context-free correctness cell outside the proposal"

[[source]]
path = ".claude/flows/mid-run-requests/review-v2.toml"
claim = "run U3 record (scope, signals, depth, changed_lines 3907, catalogue shas) and cells C18-C31"

[[source]]
path = ".review-v2-runs/.gitignore"
line = 1
claim = "run directories, including proposal.ndjson, are git-ignored"

[[source]]
path = ".claude/backlog.toml"
claim = "B-abc59fb3 and B-4e15b9fa describe the U3 and U5 proposal-vs-map rewrites"
```
