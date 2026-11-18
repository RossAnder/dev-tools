Status: done — judgement lenses slice by subsystem and file kind; signal-evidence and changed-with-tests are implemented

```toml run-note
files_touched = ["tomlctl/src/review/cells/slicing.rs", "tomlctl/src/review/cells/mod.rs", "tomlctl/src/review/cells/propose.rs", "tomlctl/tests/review_propose_slices.rs"]

[[deviations]]
what = "propose.rs changed in three places besides the slices_for call: the ScopeView constructor (ScopeView::of_run), a leave_out with reason `no scope file of its kinds` when slices_for returns nothing, and one condition on the whole-scope collapse in FloorFit::fit (the single merged slice must hold every scope file)"
why = "D9 binds the collapse to a single-kind scope, and the collapse lives in FloorFit::fit; the classified ScopeView must be built where the scope is built. Alternatives weighed: setting `whole` to None at the call site (same effect, but hides why), or skipping the collapse whenever kinds are on (would change routing-less runs). The condition chosen keeps old behaviour exactly, since per-subsystem and per-file slices used to cover every scope file"
means = "cells/propose.rs: only the call site of slices_for changes"

[[deviations]]
what = "pick_lenses treats any lens whose slices_for comes back empty as unmatched, not only per-subsystem and per-file lenses: this also covers changed-with-tests on a scope with no code, and signal-evidence when it falls back per subsystem and finds no code"
why = "otherwise such a lens would be picked, find no slice, and be reported under the misleading reason `disabled by a project scope block on every slice`; the generalisation gives the same result as the spec's rule for per-subsystem and per-file lenses"
means = "pick_lenses treats a per-subsystem or per-file judgement lens as unmatched when the scope holds no file of its kinds"

[[deviations]]
what = "Sweeper slices get kind None in split_to_cap, so the merge rule refusing cross-kind pairs only ever binds judgement slices; Slice::of sets a kind only on a kinded (code or perf) scope"
why = "Slice is built in propose.rs from plain entry lists, and split_to_cap is the one step every sweeper slice passes through, so mechanical lenses keep merging as before without touching more of propose.rs"
means = "Slice carries kind: Option<FileKind>; merge_step never joins two slices with different Some kinds"

[[tangential]]
kind = "question"
area = "tomlctl/src/review/cells/slicing.rs"
summary = "Whole-scope judgement lenses (architecture, duplication, intent-fidelity, contrarian, synthesis, doc-consolidation, external-validity) still take the run's scope glob, so a scope like `crate/**` hands them a Cargo.lock"
why = "the plan's outcome says no lockfile in a judgement cell, but its detail and D21 limit kinds to per-subsystem and per-file lenses; whether whole-scope lenses should also drop lockfiles is a call for a later plan"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check on slicing.rs, propose.rs, mod.rs and review_propose_slices.rs"
result = "pass"

[[checks]]
command = "grep -q 'SignalEvidence' tomlctl/src/review/cells/slicing.rs && grep -q 'ChangedWithTests' tomlctl/src/review/cells/slicing.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^review_propose_slices::/) | test(/^review_cells::/) | test(/^review_digest::/)'"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^review_ceiling::/) | test(/^review_finish::/) | test(/^review_plan_records::/)'"
result = "pass"
```

## What changed

- `cells/mod.rs`: `ScopeView` now holds each tracked file's `FileKind` from the run catalogue's compiled routing, plus a `kinded` flag that is off on plan-family runs. `ScopeView::of_run` builds it for both `propose` and `coverage_accounting`. When the routing has no `executable` globs, every non-lockfile counts as code. `Signals` now keeps every active signal's name, roots and evidence (`ActiveSignal`), with `./` trimmed and a `.` root dropped. `pick_lenses` treats a lens with no slice of its kinds as unmatched, the same way it treats an unmet `paths`, so accounting agrees.
- `cells/slicing.rs`: `slices_for` now takes the lens `Entry`.
  - On a kinded scope, a per-subsystem or per-file judgement lens groups by (key, kind) and keeps only its `kinds` (code when it declares none), so lockfiles never enter.
  - `signal-evidence` slices the in-scope evidence files and files under the roots of the lens's matched active signals. It includes every kind, gives one slice per subsystem, and falls back to kind-aware per-subsystem when no signal names a file.
  - `changed-with-tests` slices, per subsystem holding changed code, the code files plus the tracked test files under the same subsystem.
  - On plan runs both new rules act as per-subsystem, and kinds are ignored.
  - `Slice` gains `kind`, and `merge_step` refuses to join two slices with different `Some` kinds. `compact_tree` and `compact_dir` are unchanged and still compare against all tracked files.
- `cells/propose.rs`: changed at the `slices_for` call site, the `ScopeView` constructor and the collapse condition (see the deviations).
- `tests/review_propose_slices.rs`: six tests.
  - A mixed scope gives correctness a code slice and a prose slice, with no lockfile and no covering `dir/**`.
  - Security slices to its evidence file, and falls back per subsystem when the signal has no files.
  - Test-gaps holds the changed code plus the subsystem's tests and no markdown.
  - A `kinds = ["test"]` lens is unmatched on a scope with no tests, both in the proposal and in accounting, and is proposed over the tests once a test is in scope.
  - A routing-less catalogue slices every kind together, as before.
  - A plan run over markdown still proposes signal-evidence and changed-with-tests cells.

`review_cells.rs` was not edited: its existing tests pass unchanged on the routing-less fallback.

One compile attempt failed on a sibling's in-flight edit to `tomlctl/src/review/stats.rs` (`yields` not yet defined). It passed once that edit landed.
