Status: done — the U3 and U5 replays assert the proposal criteria against the live catalogue, and two proposal defects they exposed are fixed under granted request Q1

```toml run-note
files_touched = ["tomlctl/tests/review_replay.rs", "tomlctl/tests/fixtures/review_replay/u3/run.toml", "tomlctl/tests/fixtures/review_replay/u3/tracked.txt", "tomlctl/tests/fixtures/review_replay/u5/run.toml", "tomlctl/tests/fixtures/review_replay/u5/tracked.txt", "tomlctl/src/review/cells/slicing.rs", "tomlctl/src/review/cells/propose.rs"]

[[deviations]]
what = "U5 tokens preserve sort order. Every original name (directory segment or whole file name) is ranked against every other name and against the kept routing names present in the tree. Its token is a short prefix that sorts between the neighbouring kept names, then a fixed-width base-36 rank, then the kept suffix (extension, or the .test/.spec infix and extension)"
why = "With plain first-seen tokens, the anonymised and un-anonymised U5 replays proposed the same cell count but different concerns per cell. Slicing groups and merge tie-breaks follow path order, so the confirmation the detail requires failed. Order-preserving tokens make the two replays identical: the same cells, concerns, entry counts and unselected list"
means = "detail: map each path segment to a stable token"

[[deviations]]
what = "The fixtures were written by a throwaway generator script in the session scratchpad, with explicit LF line endings, not by the Write tool"
why = "The tracked lists hold 2366 and 3249 paths, and the detail asks for a script to anonymise U5. The script read the U3 and U5 run records with tomllib and git ls-tree at c7c85ae9 and d1930996. It also wrote an un-anonymised U5 copy outside the repo for the equivalence check"
means = "file edits through Edit and Write"

[[deviations]]
what = "The fixture loader builds its own git sandbox from common::sandbox(), removes the sandbox .gitignore and runs `git add -A -f`, rather than using review_harness::Repo::new"
why = "Repo::new writes the synthetic fixture catalogue and adds without -f. A tracked path matched by the shipped ignore rules would then drop out of the index, and the sandbox's own .gitignore would add a path the run's head never had. The test asserts that ls-files equals tracked.txt"
means = "D17: load into the existing git sandbox"

[[deviations]]
what = "slicing.rs merge_step: under a non-adjacent rule ('any') it now considers every pair i<j, not only consecutive ones. The merged slice takes the earlier index"
why = "Granted under request Q1. Judgement slices alternate code, other, code, other in (subsystem, kind) order. Merging only consecutive pairs could never join two slices of one kind across a slice of the other. On U5, FloorFit returned None and correctness, a floor lens, was cut for the ceiling"
means = "the merge ladder's 'any' rule"

[[deviations]]
what = "propose.rs: before the judgement pass, the judgement lenses are stably re-ranked into floor, then forced, then signal-matched, then the rest. Among the signal-matched lenses, the one covering the fewest scope files goes first"
why = "Granted under request Q1, which asked for a catalogue-driven order with no lens-id branch and no change to any ceiling or cap. On U5, domain-model, interaction and ui-visual took every judgement cell before security, so security (three trust-boundary evidence files) was cut. Alternatives: hold back one cell per later matched lens, like the sweeper hold (more cuts overall, since roughly ten lenses matched on U5), or spread the room evenly (same problem). Narrowest-first admits the narrowly pointed lens and lets the broad per-subsystem lenses merge down into what is left"
means = "J phase in today's tier order"

[[tangential]]
kind = "debt"
area = "tomlctl/src/review/cells/slicing.rs"
summary = "merge_step now builds and joins every candidate pair before sorting, which is O(n^2) joins per step over a lens's slices"
why = "The full proposal tests take a few seconds in debug builds. A scope with hundreds of subsystems could make propose slow, and computing sizes before joining would avoid it"

[[tangential]]
kind = "question"
area = "tomlctl/src/review/cells/propose.rs"
summary = "On U5 the reworked proposal still cuts doc-drift, external-validity, architecture and hygiene for the ceiling, and ui-visual, type-design and seams on the un-anonymised ordering"
why = "These lenses are signal-matched. With no repeat proposed, the criteria allow the cuts, but the coverage loss on a 30k-line diff is a call for the ceiling retune this plan defers"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/review_replay.rs tomlctl/src/review/cells/slicing.rs tomlctl/src/review/cells/propose.rs"
result = "pass"

[[checks]]
command = "test -s tomlctl/tests/fixtures/review_replay/u3/tracked.txt && test -s tomlctl/tests/fixtures/review_replay/u5/tracked.txt && test -s tomlctl/tests/fixtures/review_replay/u5/run.toml"
result = "pass"

[[checks]]
command = "test -s tomlctl/tests/fixtures/review_replay/u5/tracked.txt && test -s tomlctl/tests/fixtures/review_replay/u5/run.toml && ! grep -qi 'reportdesign\\|switchDocuments\\|package-design' tomlctl/tests/fixtures/review_replay/u5/tracked.txt tomlctl/tests/fixtures/review_replay/u5/run.toml"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^review_replay::/) | test(/^review_propose_admission::/) | test(/^review_propose_slices::/) | test(/^review_cells::/) | test(/^review_ceiling::/)'"
result = "pass"
```

## What changed

- `tomlctl/tests/review_replay.rs` has three tests.
  - **Loader.** Each fixture is loaded into a git sandbox of empty tracked files. The run record is wrapped in a one-run store whose `catalogue = { builtin = <repo>/claude/skills/review-v2-lenses }` carries no shas. The test runs `review plan --propose --lens +intent-fidelity --context-free --reserve 1`.
  - **Expansion and kinds.** Each slice entry expands against `tracked.txt` by PathSet's rules: globset with `literal_separator`, and a plain entry also matches everything under it. Files are classified by the live `vocabulary.toml` `[routing]` lists, checked in the order lockfiles, tests, executable, and anything else is other.
  - **Signal matching.** A lens counts as signal-matched from its frontmatter: activation `signal` or `candidate`, with an `apply_when` signal active in the run.
  - **Assertions.** Every property listed in the detail is asserted for both runs, and for U5 that security is admitted.
  - **Kind coverage.** A third test checks that the anonymised U5 tree still holds lockfile, test, code and other files.
- **U3 fixture.** `u3/run.toml` holds run U3's record verbatim: id, family, depth, changed_lines, scope and signals, with roots, evidence and accepted. It has no cells, outcomes or catalogue shas. `u3/tracked.txt` is `git ls-tree -r --name-only c7c85ae9`, 2366 paths.
- **U5 fixture.** `u5/run.toml` and `u5/tracked.txt` (3249 paths) hold U5's record and head list, anonymised.
  - **Kept as written:** `src`, `tests`, `test`, `__tests__`, `docs`, `.githooks`, `*Tests`/`*Test` segments, lockfile and manifest names, and the `.test`/`.spec` infix with its extension.
  - **Mapped:** everything else, the same way in tracked paths, scope, signal roots and evidence.
  - **Equivalence.** The anonymised and un-anonymised replays give the same proposal: 15 cells, the same concerns per cell, the same entry counts and the same unselected list.
- **`slicing.rs` (granted Q1).** `merge_step`'s non-adjacent rules merge any same-kind pair, not only consecutive ones.
- **`propose.rs` (granted Q1).** The judgement pass ranks signal-matched lenses ahead of unmatched ones, the fewest scope files first.
- **U3 result.** 15 cells. doc-drift is now admitted, and only hygiene is cut.
- **U5 result.** 15 cells. correctness is admitted, with 3 context-free cells, and security holds 2 cells.

Request Q1 asked for two paths, `tomlctl/src/review/cells/slicing.rs` and `tomlctl/src/review/cells/propose.rs`. Both were granted.

I ran the replay test and the proposal test modules myself, although the view marks the replay accept check `Run by: gate`. The changes to two finished tasks' files needed proof that their tests still pass: 44 of 44 passed.
