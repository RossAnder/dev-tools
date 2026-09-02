# Plan: tomlctl follow-ups from the backlog sweep

**Plan path**: `docs/plans/tomlctl-followups.md`
**Created**: 2026-09-02
**Status**: Draft

## Context

The 2026-09-02 `/backlog` sweep closed the open set: 18 items were promoted here and one
question (`B-2fa6e384`, the `*.premerge.md` ignore) resolved as intended. The 18 fall into four
groups — self-gating tests that pass without covering what they claim, two shipped-behaviour
bugs, internal duplication, and hygiene debt that hides new signal among old noise.

Two of them turned out to be far larger than their capture text implied, and exploration
measured both before this plan was written:

- **`B-7269164c` (no rustfmt gate)** — `cargo fmt --check` reports diffs in **52 files** across
  `tomlctl/src/` and `tomlctl/tests/`. Every one of the nine files the other items edit is
  among them, so the reformat cannot run concurrently with anything.
- **`B-90c6b8fe` (finding ids in source comments)** — captured as three files; the pattern
  actually hits **509 comment lines across 29 files** under `tomlctl/src/`. A further 272 hits
  are legitimate test-fixture literals (`id = "R1"`, `.arg("R999")`) that must not be touched.

The intended outcome is a crate where the gates gate: formatting enforced at commit time,
clippy warnings at zero and denied at the manifest, the markdown/carrier lint tests actually
covering the invocations they scan, and the shared-block verifier unable to pass vacuously.

## Scope

- **In scope**: all 18 promoted backlog items; the whole-crate reformat and the pre-commit fmt
  gate that keeps it; the crate-wide finding-id comment sweep across `tomlctl/src/` and
  `tomlctl/Cargo.toml`; deletion of the vacuous `blocks verify-skills` verb; correcting
  `CLAUDE.md`'s claim that the drift tests are gated by CI.
- **Out of scope**: building CI (there is no `.github/` directory and the gate home chosen is
  the pre-commit hook); anchor resolution in the new markdown link checker (file existence
  only); changing the documented `null`-does-not-clear semantics of `items update`; the 133
  finding-id comment hits under `tomlctl/tests/` (see Risks — this needs a separate decision);
  adding a `[lib]` target to the crate.
- **Affected areas**: `tomlctl/src/`, `tomlctl/tests/`, `tomlctl/Cargo.toml`,
  `tomlctl/README.md`, `scripts/`, `.githooks/`, `claude/skills/tomlctl/references/`,
  `CLAUDE.md`, repository root.

## Research Notes

**`cargo fmt` ignores path arguments — the capture is correct.**
`cargo fmt -- --check <path>` and bare `cargo fmt --check` both report the same crate-wide
diff; anything after `--` is forwarded as rustfmt *options* and a positional path is swallowed.
`-p <package>` narrows nothing in a single-package repo. Source:
[cargo-fmt README](https://github.com/rust-lang/rustfmt/blob/master/README.md), reproduced
locally. Evidence: high.

**`rustfmt --edition 2024 <path>` is the escape hatch, but it recurses into `mod` children.**
`rustfmt --edition 2024 --check tomlctl/src/main.rs` reports 267 sites across the whole module
tree. Only *leaf* modules are safely file-scoped; `--skip-children` is nightly-only.
`--edition 2024` is mandatory — direct rustfmt defaults to edition 2015. This qualifies the
advice recorded in `B-f37b7739`. Evidence: high.

**`[lints.clippy]` groups need a negative priority.**
`pedantic = { level = "warn", priority = -1 }` then `manual_filter = "deny"`. A group left at
the default priority 0 alongside specific lints is a same-priority conflict that Cargo rejects.
Source: [Cargo manifest reference — the lints
section](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section). Lints
reach `tomlctl/tests/` targets only under `cargo clippy --all-targets`. Evidence: high
(target granularity inferred: medium).

**`.git-blame-ignore-revs` cannot contain its own commit's SHA.**
The correct sequence is: commit the reformat, read its SHA, then commit a separate append. The
file is read at blame time, so it applies retroactively and the ordering costs nothing. It must
sit at the *repository* root to be honoured automatically by GitHub; local use needs a per-clone
`git config blame.ignoreRevsFile .git-blame-ignore-revs`, which cannot be committed. Sources:
[git-blame](https://git-scm.com/docs/git-blame),
[GitHub discussion #5033](https://github.com/orgs/community/discussions/5033). Evidence: high.

**A whole-crate `cargo fmt --check` costs 0.9s.**
Measured on this crate under Git Bash. rustfmt parses but does not compile, so cost scales with
source bytes, not the dependency graph — no contention with the shared `tomlctl/target` lock. A
staged-files-only variant would reintroduce the module-recursion caveat and miss unstaged
drift, so it buys nothing. The hook must pass `--manifest-path tomlctl/Cargo.toml`; the repo
root has no package. Evidence: high.

**A `[lib]` target would not fix the duplicated `shipped_gitignore`.**
The helper sits inside `#[cfg(test)] mod tests` at `tomlctl/src/backlog/evidence_ops.rs`
(`mod tests` opens at line 489, `fn shipped_gitignore` at line 510). Cargo compiles the lib
without `cfg(test)` for integration tests to link against, so the item stays invisible — a
scratch probe reproduced this as `error[E0425]`. Evidence: high.

**The "HTML markers are invalid in Rust" blocker is false.**
`scripts/verify-shared-blocks.sh` (`hash_block`, lines 32-40) and `tomlctl/src/blocks.rs` both
match markers on exact line equality, which a `/* … */` block comment satisfies verbatim. A
probe confirmed `rustc --edition 2024` accepts the framing and the existing awk extracts the
region correctly. Evidence: high.

**The two `shipped_gitignore` copies are not byte-identical today.**
The 15 code lines match once four spaces of `mod tests` indentation are stripped, but the doc
comments differ in wording and wrap width. Any identity gate needs the comments unified first.
rustfmt will not do this — `wrap_comments` is nightly-only and off by default, and no
`rustfmt.toml` exists anywhere in the repo. Evidence: high.

**`include!` files are invisible to `cargo fmt`.**
A probe confirmed rustfmt walks the `mod` tree and never visits an `include!`d file, which would
create a permanently unformatted island escaping the very gate this plan adds. This rules out
the `include!` approach to sharing the helper. Evidence: high.

**There is no CI in this repository.**
No `.github/` directory exists, and no GitLab/Azure/Jenkins equivalent. `CLAUDE.md`'s statement
that the drift tests are "CI only — the pre-commit hook does not run these" describes an intent,
not a running system. Evidence: high.

## User Decisions

**Finding-id sweep scope** — *Full 29-file sweep.* Prompted by the measurement that the capture's
three named files hold ~134 of 781 raw hits, while the rule at `.claude/rules/documentation.md`
line 26 is violated 509 times across 29 files.

**Reformat position** — *Sweep first, then fixes.* Prompted by the measurement that the reformat
overlaps every file the other 17 items edit; landing fixes first would split each fix's blame
across two commits when the sweep re-touched its lines.

**Fmt gate home** — *Pre-commit hook, blocking.* Prompted by the discovery that no CI exists, so
the hook and a non-existent workflow were the only two candidates.

**`blocks verify-skills`** — *Delete the verb.* Prompted by `scripts/shared-blocks.toml` carrying
two entries that use only `{name, files}`, with the manifest itself documenting `skill` as "now
unused" — the verb is live code over an empty input set.

**`dispatch.rs` contention** — *Split the test module first.* Prompted by five separate items
all editing `tomlctl/src/cli/dispatch.rs`, whose inline `#[cfg(test)] mod tests` spans lines
1241-2209 of 2209.

**`items update` scope** — *Fix the required-`--json` half only.* Prompted by the discovery that
the `null`-does-not-clear behaviour is documented as intentional in `tomlctl/src/items.rs`
(the rationale block above `is_empty_json`), directing callers to `--unset`.

**Link checker ambition** — *File existence only.* Prompted by the corpus holding 94 anchor
links whose slugs include double-hyphen forms, where an imperfect slug algorithm would produce
false failures.

**`shipped_gitignore` de-duplication** — *Shared-block markers.* Prompted by research
overturning both premises in the capture: a `[lib]` target does not expose `cfg(test)` items,
and HTML markers work fine inside Rust block comments.

**Checkpoint cadence** — *Milestones.* Prompted by the plan reaching 34 tasks across four
naturally-bisectable boundaries.

**`CLAUDE.md` CI claim** — *Fix it in the fmt-gate task.* Prompted by exploration finding no
`.github/` directory; the fmt-gate task already edits the same build section.

## Approach

The plan is ordered so that the two crate-wide mechanical sweeps land before any substantive
edit, and the gates that keep them are armed immediately after.

**Foundation first.** The reformat (task 1) runs alone, then the clippy baseline is driven to
zero (task 2) and denied at the manifest (task 3) so a new warning can never again read as
pre-existing noise. Task 4 extracts the inline test module from
`tomlctl/src/cli/dispatch.rs` — a move-only change that converts the plan's biggest
serialisation bottleneck into four independently-dispatchable tasks. Task 5 arms the pre-commit
fmt gate. Everything downstream therefore edits already-formatted, already-gated code.

**The finding-id sweep is partitioned by file, not by meaning.** Each of tasks 7-20 owns a
disjoint file set, so they run in parallel up to the declared maximum. Every one carries the
same hazard note: the pattern also matches test-fixture literals, and the sweep is
comment-lines-only. Where a comment states a real invariant, the invariant replaces the id;
where it states nothing beyond provenance, the comment goes.

**De-duplication reuses what already exists rather than adding machinery.** `strict_read_check`
is promoted to `pub(crate)` and called from its two existing copies rather than being
re-implemented a third time. `shipped_gitignore` moves to `tomlctl/src/test_support.rs` — a
module the crate already declares `#[cfg(test)]` in `tomlctl/src/main.rs` and already
describes as the home for shared test scaffolding — and is then gated by the existing
`scripts/verify-shared-blocks.sh` via one new `[[block]]` entry. No new test binary, no
`build.rs`, no `[lib]` target, no `include!`.

**The non-empty assertion precedes the new block entry.** Task 25 hardens
`scripts/verify-shared-blocks.sh` before task 33 gives it a new block to check. In the current
script a CR-preserving awk makes every block extract to the empty string, both sides hash to
`e3b0c442…b855`, and parity prints `OK` — so adding a block first would arm a gate that can
pass vacuously.

Rejected: adding a `[lib]` target (does not expose `cfg(test)` items, and would silently turn
task 2's `cargo clippy --fix --bin tomlctl` into a no-op); a bespoke byte-identity test (only
runs under `cargo test`, which nothing automates); a build script (disproportionate for a
26-line helper, and its `rerun-if-changed` would point outside the package root).

## Verification Commands

```
build: cargo build --manifest-path tomlctl/Cargo.toml
test: cargo test --manifest-path tomlctl/Cargo.toml
lint: cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets
```

Additional gates this plan introduces or repairs, run from the repository root:

- `cargo fmt --manifest-path tomlctl/Cargo.toml -- --check` — must exit 0 from task 1 onward.
- `bash scripts/verify-shared-blocks.sh` — must exit 0, and from task 25 must fail loudly rather
  than pass silently when a block extracts empty.
- `cargo audit --file tomlctl/Cargo.lock` — final pass only.

Manual check after task 6: run `git config blame.ignoreRevsFile .git-blame-ignore-revs`, then
`git blame tomlctl/src/items.rs` and confirm lines are attributed to their substantive commits
rather than to the reformat.

## Execution Policy

- **Checkpoints**: milestones
- **Checkpoint after**: tasks 5, 20, 25, 34
- **Max parallel agents**: 8
- **Commit granularity**: per-task

## Tasks

### Wave 0 — foundation (serial)

### 1. Reformat the tomlctl crate [S]
- **Files**: every `*.rs` under `tomlctl/src/` and `tomlctl/tests/` that rustfmt rewrites —
  derive the set with `cargo fmt --manifest-path tomlctl/Cargo.toml -- --check` before starting
  (52 files at time of planning; do not transcribe the list).
- **Depends on**: —
- **Action**: Run `cargo fmt --manifest-path tomlctl/Cargo.toml` and commit the result as a
  single formatting-only change.
- **Detail**: No `rustfmt.toml` exists in the repository, so this runs on stock defaults
  (`max_width = 100`). Make no other edit in this task — the commit must contain nothing but
  formatting, because task 6 will record its SHA as blame-ignorable. Do not add a `rustfmt.toml`.
- **Acceptance**: `cargo fmt --manifest-path tomlctl/Cargo.toml -- --check` exits 0, and
  `cargo build --manifest-path tomlctl/Cargo.toml` succeeds. `git diff --stat` for this task
  shows only `*.rs` files.

### 2. Auto-fix the six pre-existing clippy warnings [S]
- **Files**: `tomlctl/src/io.rs`, `tomlctl/src/capabilities.rs`
- **Depends on**: 1
- **Action**: Clear the five `manual_filter` warnings in `tomlctl/src/io.rs` and the one
  `unnecessary_map_or` in `tomlctl/src/capabilities.rs`.
- **Detail**: All five `io.rs` sites share the shape
  `.parent().and_then(|p| if p.as_os_str().is_empty() { None } else { Some(p) })` — inside
  `recheck_claude_containment`, `ensure_parent_under_claude`, `atomic_write` and two siblings.
  `cargo clippy --fix --bin tomlctl` handles all six; review the result rather than trusting it,
  and re-run `cargo fmt` on the two files afterwards since `--fix` output is not formatted.
- **Acceptance**: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` emits zero
  warnings for `tomlctl/src/io.rs` and `tomlctl/src/capabilities.rs`. Fails if any warning
  remains in either file.

### 3. Deny the two fixed lints at the manifest [S]
- **Files**: `tomlctl/Cargo.toml`
- **Depends on**: 2
- **Action**: Add a `[lints.clippy]` table beside the existing `[lints.rustdoc]` table denying
  `manual_filter` and `unnecessary_map_or`.
- **Detail**: Use the bare-string form (`manual_filter = "deny"`), which is shorthand for
  `{ level = "deny", priority = 0 }`. Do not add a lint *group* here; if one is ever added it
  must carry `priority = -1` or Cargo rejects the same-priority conflict. Add a comment stating
  why these two are denied, in the style of the existing `[lints.rustdoc]` comment — without
  citing any backlog or finding id.
- **Acceptance**: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` exits 0.
  Reintroducing a `manual_filter` pattern in `tomlctl/src/io.rs` makes it exit non-zero.

### 4. Extract the dispatch.rs test module into per-concern modules [M]
- **Files**: `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/cli/dispatch/tests/mod.rs`,
  `tomlctl/src/cli/dispatch/tests/lint.rs`, `tomlctl/src/cli/dispatch/tests/skills.rs`
- **Depends on**: 1
- **Action**: Move the inline `#[cfg(test)] mod tests` block (currently the tail of
  `tomlctl/src/cli/dispatch.rs`, opening after the production half) into sibling modules, so the
  markdown-gating tests stop contending on one file.
- **Detail**: Move only — change no test body. Split by concern: `command_lint` and
  `command_lint_scan_set` into `lint.rs`; `carrier_invokes_required_skills`,
  `skill_bodies_under_line_ceiling` and `skill_references_under_line_ceiling` plus the dormant `verify_skills_clean` into `skills.rs`;
  `mod.rs` declares both and re-exports whatever the moved tests share. The block's existing
  header imports (`use super::*;` plus `crate::blocks`, `std::fs`, `std::path`) move with it;
  `use super::*` becomes `use crate::cli::dispatch::*`. Every moved test derives the repository
  root the same way it does today, from `CARGO_MANIFEST_DIR`'s parent.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml` runs the same test count as
  before the move and all pass. `tomlctl/src/cli/dispatch.rs` contains no `#[cfg(test)]` block.
  Fails if any test is dropped or renamed.

### 5. Arm the pre-commit fmt gate and correct the developer documentation [M]
- **Files**: `.githooks/pre-commit`, `.git-blame-ignore-revs`, `CLAUDE.md`
- **Depends on**: 1
- **Action**: Add a blocking `cargo fmt --check` step to the pre-commit hook, create an empty
  `.git-blame-ignore-revs` at the repository root, and fix two inaccurate claims in `CLAUDE.md`.
- **Detail**: Add the fmt step after the three existing verifiers in `.githooks/pre-commit`,
  guarded so it runs only when the staged set contains a `*.rs` path — the hook already computes
  the staged list and early-exits when it is empty. It must pass
  `--manifest-path tomlctl/Cargo.toml`; the repository root has no package and a bare invocation
  fails. Under the hook's `set -euo pipefail` a non-zero exit blocks the commit, which is the
  intent. Create `.git-blame-ignore-revs` containing only a header comment (task 6 appends the
  SHA). In `CLAUDE.md`: correct the statement that the drift tests are gated by CI — no
  `.github/` directory exists, so they run only when invoked by hand — and add the
  `git config blame.ignoreRevsFile .git-blame-ignore-revs` step beside the existing
  `core.hooksPath` instruction. Also record the `cargo fmt` path-argument gotcha and the
  `rustfmt --edition 2024` module-recursion caveat in the build-tuning notes.
- **Acceptance**: Staging a deliberately misformatted `*.rs` file and running
  `bash .githooks/pre-commit` exits non-zero and names the file; staging a non-Rust file only
  leaves the fmt step unrun. `grep -c 'CI only' CLAUDE.md` returns 0.

### Wave 1 — blame provenance and the finding-id sweep

Tasks 7-20 are file-disjoint and dispatch in parallel. Each removes finding ids, task refs and
review-round refs from **comment lines only**, per `.claude/rules/documentation.md`. The same
pattern matches test-fixture literals (`id = "R1"`, `.arg("R999")`, `"no item with id = R999"`)
and those must be left untouched. Where a comment carries a real invariant, restate the
invariant and drop the id; where it carries only provenance, delete the comment.

### 6. Record the reformat commit as blame-ignorable [S]
- **Files**: `.git-blame-ignore-revs`
- **Depends on**: 5
- **Action**: Append the full 40-character SHA of task 1's formatting commit to
  `.git-blame-ignore-revs`.
- **Detail**: A commit cannot contain its own SHA, so this must be a separate commit after task
  1's has landed — derive the SHA with `git log` rather than hard-coding one, and add a `#`
  comment line above it saying what the commit was. One unabbreviated SHA per line.
- **Acceptance**: The file contains exactly one 40-hex-character line, and
  `git blame --ignore-revs-file .git-blame-ignore-revs tomlctl/src/items.rs` attributes lines to
  commits other than the reformat. Fails if the SHA is abbreviated or names a different commit.

### 7. Sweep finding-id comments from items.rs [M]
- **Files**: `tomlctl/src/items.rs`
- **Depends on**: 1
- **Action**: Remove or rewrite the 93 comment-line finding-id references in this file.
- **Detail**: This is the densest file in the sweep and also holds TOML test fixtures whose
  `id = "R…"` values are data — restrict every edit to lines whose first non-whitespace
  characters are `//`, `///`, `//!` or a block-comment continuation. The rationale block above
  `is_empty_json` is a good example of a comment to keep while dropping its id.
- **Acceptance**: `grep -nE '\b[RO][0-9]+\b' tomlctl/src/items.rs` returns no line that is a
  comment. `cargo test --manifest-path tomlctl/Cargo.toml` still passes, proving no fixture was
  altered.

### 8. Sweep finding-id comments from io.rs [M]
- **Files**: `tomlctl/src/io.rs`
- **Depends on**: 2
- **Action**: Remove or rewrite the 89 comment-line finding-id references in this file.
- **Detail**: Depends on task 2 because that task rewrites five call sites in this file; sweeping
  first would put the two tasks in conflict.
- **Acceptance**: `grep -nE '\b[RO][0-9]+\b' tomlctl/src/io.rs` returns no comment line.
  `cargo build --manifest-path tomlctl/Cargo.toml` succeeds.

### 9. Sweep finding-id comments from query.rs [M]
- **Files**: `tomlctl/src/query.rs`
- **Depends on**: 1
- **Action**: Remove or rewrite the 81 comment-line finding-id references in this file.
- **Acceptance**: `grep -nE '\b[RO][0-9]+\b' tomlctl/src/query.rs` returns no comment line.
  `cargo test --manifest-path tomlctl/Cargo.toml` still passes.

### 10. Sweep finding-id comments from cli/dispatch.rs [M]
- **Files**: `tomlctl/src/cli/dispatch.rs`
- **Depends on**: 4
- **Action**: Remove or rewrite the 40 comment-line finding-id references in this file.
- **Detail**: Depends on task 4, which moves the test module out of this file — sweeping first
  would conflict with the move and some of the 40 hits may travel with it. Re-derive the hit set
  after task 4 lands rather than working from the planning-time count.
- **Acceptance**: `grep -nE '\b[RO][0-9]+\b' tomlctl/src/cli/dispatch.rs` returns no comment
  line. `cargo build --manifest-path tomlctl/Cargo.toml` succeeds.

### 11. Sweep finding-id comments from cli/types.rs and cli/mod.rs [M]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/mod.rs`
- **Depends on**: 1
- **Action**: Remove or rewrite the 31 comment-line finding-id references across these files.
- **Detail**: `tomlctl/src/cli/types.rs` holds the `SUBCOMMANDS` constant whose doc comment says
  it is kept in sync by hand — keep that instruction, drop any id attached to it.
- **Acceptance**: `grep -nE '\b[RO][0-9]+\b'` over both files returns no comment line.
  `cargo build --manifest-path tomlctl/Cargo.toml` succeeds.

### 12. Sweep finding-id comments from flow/resolve.rs and dedup.rs [M]
- **Files**: `tomlctl/src/flow/resolve.rs`, `tomlctl/src/dedup.rs`
- **Depends on**: 1
- **Action**: Remove or rewrite the 42 comment-line finding-id references across these files.
- **Detail**: `tomlctl/src/dedup.rs` holds a large TOML fixture block whose `id = "R…"` entries
  are data, not references.
- **Acceptance**: `grep -nE '\b[RO][0-9]+\b'` over both files returns no comment line.
  `cargo test --manifest-path tomlctl/Cargo.toml` still passes.

### 13. Sweep finding-id comments from orphans.rs and convert.rs [M]
- **Files**: `tomlctl/src/orphans.rs`, `tomlctl/src/convert.rs`
- **Depends on**: 1
- **Action**: Remove or rewrite the 38 comment-line finding-id references across these files.
- **Acceptance**: `grep -nE '\b[RO][0-9]+\b'` over both files returns no comment line.
  `cargo test --manifest-path tomlctl/Cargo.toml` still passes.

### 14. Sweep finding-id comments from three flow modules [M]
- **Files**: `tomlctl/src/flow/render_progress_log.rs`, `tomlctl/src/flow/init.rs`,
  `tomlctl/src/flow/active.rs`
- **Depends on**: 1
- **Action**: Remove or rewrite the 37 comment-line finding-id references across these files.
- **Acceptance**: `grep -nE '\b[RO][0-9]+\b'` over the three files returns no comment line.
  `cargo build --manifest-path tomlctl/Cargo.toml` succeeds.

### 15. Sweep finding-id comments from output.rs, flow/doctor.rs and flow/schema.rs [S]
- **Files**: `tomlctl/src/output.rs`, `tomlctl/src/flow/doctor.rs`, `tomlctl/src/flow/schema.rs`
- **Depends on**: 1
- **Action**: Remove or rewrite the 23 comment-line finding-id references across these files.
- **Acceptance**: `grep -nE '\b[RO][0-9]+\b'` over the three files returns no comment line.
  `cargo build --manifest-path tomlctl/Cargo.toml` succeeds.

### 16. Sweep finding-id comments from time.rs, blocks.rs and main.rs [S]
- **Files**: `tomlctl/src/time.rs`, `tomlctl/src/blocks.rs`, `tomlctl/src/main.rs`
- **Depends on**: 1
- **Action**: Remove or rewrite the 14 comment-line finding-id references across these files.
- **Detail**: `tomlctl/src/blocks.rs` carries comments describing the `verify-skills` drift
  engine, which task 34 deletes outright — sweep what survives and leave the deletion to that
  task rather than pre-empting it.
- **Acceptance**: `grep -nE '\b[RO][0-9]+\b'` over the three files returns no comment line.
  `cargo build --manifest-path tomlctl/Cargo.toml` succeeds.

### 17. Sweep finding-id comments from integrity.rs and two flow modules [S]
- **Files**: `tomlctl/src/integrity.rs`, `tomlctl/src/flow/ensure_artifact.rs`,
  `tomlctl/src/flow/envelope.rs`
- **Depends on**: 1
- **Action**: Remove or rewrite the 11 comment-line finding-id references across these files.
- **Acceptance**: `grep -nE '\b[RO][0-9]+\b'` over the three files returns no comment line.
  `cargo build --manifest-path tomlctl/Cargo.toml` succeeds.

### 18. Sweep finding-id comments from json.rs, flow/list.rs and flow/artifacts.rs [S]
- **Files**: `tomlctl/src/json.rs`, `tomlctl/src/flow/list.rs`, `tomlctl/src/flow/artifacts.rs`
- **Depends on**: 1
- **Action**: Remove or rewrite the 6 comment-line finding-id references across these files.
- **Detail**: `tomlctl/src/json.rs` holds a duplicate `strict_read_check` whose doc comment says
  it mirrors the `cli::dispatch` original — keep that cross-reference in prose, since task 32
  removes the duplication entirely and will revisit the comment.
- **Acceptance**: `grep -nE '\b[RO][0-9]+\b'` over the three files returns no comment line.
  `cargo build --manifest-path tomlctl/Cargo.toml` succeeds.

### 19. Sweep finding-id comments from the four remaining single-hit modules [S]
- **Files**: `tomlctl/src/flow/stale.rs`, `tomlctl/src/flow/mod.rs`,
  `tomlctl/src/flow/find_plans.rs`, `tomlctl/src/capabilities.rs`
- **Depends on**: 2
- **Action**: Remove or rewrite the four remaining comment-line finding-id references.
- **Detail**: Four files rather than three because each holds exactly one hit and the edits are
  one line apiece. Depends on task 2 because that task edits `tomlctl/src/capabilities.rs`.
- **Acceptance**: `grep -rnE '\b[RO][0-9]+\b' tomlctl/src --include='*.rs'` returns no line whose
  first non-whitespace characters begin a comment, across the whole directory.

### 20. Sweep finding-id comments from the crate manifest [S]
- **Files**: `tomlctl/Cargo.toml`
- **Depends on**: 3
- **Action**: Remove the four finding-id and task-ref citations from the dependency comments.
- **Detail**: The comments explaining `preserve_order`, the mimalloc choice, the `assert_cmd`
  harness and `shell-words` each carry an id. Keep every rationale — they are the reason each
  dependency is pinned as it is — and drop only the identifiers. Depends on task 3, which adds
  the `[lints.clippy]` table to this file.
- **Acceptance**: `grep -nE '\b[RO][0-9]+\b|\bT[0-9]+\b' tomlctl/Cargo.toml` returns nothing.
  `cargo build --manifest-path tomlctl/Cargo.toml` succeeds.

### Wave 2 — gate hardening

### 21. Teach command_lint to recognise angle-bracket placeholders [M]
- **Files**: `tomlctl/src/cli/dispatch/tests/lint.rs`
- **Depends on**: 4
- **Action**: Stop truncating an argv at the first `<placeholder>` token so flags after it are
  linted.
- **Detail**: The `is_shell_op` closure inside `command_lint`'s `lint_logical` treats any token
  starting with `<` as shell plumbing, so the `take_while` that follows drops everything after a
  placeholder. Redirections that must still truncate are `<<`, `2>`, `1>`, `>` and a bare `<`.
  Distinguish a placeholder — a token matching `<…>` with a closing bracket — and substitute a
  parseable dummy value so the remaining flags reach `Cli::try_parse_from`.
- **Acceptance**: A fixture line of the form `tomlctl flow init --slug <slug> --bogus-flag`
  fails `command_lint`, where today it passes. `cargo test --manifest-path tomlctl/Cargo.toml`
  passes with no new failures in the real carrier corpus — any failure it surfaces is a genuine
  drift and must be reported, not silenced.

### 22. Tighten carrier_invokes_required_skills to a phrase match [M]
- **Files**: `tomlctl/src/cli/dispatch/tests/skills.rs`
- **Depends on**: 4
- **Action**: Replace the bare-substring skill check with one that requires an actual invocation
  phrase.
- **Detail**: Both the carrier check and its plugin mirror currently test whether the file text
  merely *contains* the skill name, so any prose mention — including a sentence saying not to use
  it — satisfies the assertion. Require either the "Invoke the `<name>` skill" phrasing the
  carriers already use, or a backtick-quoted name followed by the word "skill". Keep the existing
  cross-check that each required `SKILL.md` exists on disk.
- **Acceptance**: A fixture carrier whose only mention of a required skill is a negative sentence
  fails the test. The real carrier corpus still passes `cargo test --manifest-path
  tomlctl/Cargo.toml`; a failure there is genuine drift to report rather than to accommodate.

### 23. Add a markdown link-existence checker [M]
- **Files**: `tomlctl/src/cli/dispatch/tests/skills.rs`
- **Depends on**: 22
- **Action**: Add a test asserting every relative markdown link target under `claude/skills/`
  resolves to a file that exists.
- **Detail**: Model it on the existing `skill_references_under_line_ceiling`, which already walks
  one level into each `references/` directory. Resolve relative paths and `../` traversal
  against the linking file's directory. Ignore the fragment of any link that carries one, and
  ignore pure-anchor links entirely — anchor resolution is explicitly out of scope, and the
  corpus holds 94 anchor links whose slugs would need GitHub's exact algorithm. Report every
  broken link in one panic rather than failing on the first.
- **Acceptance**: A fixture link to a non-existent sibling file fails the test with that path
  named. The real corpus passes; a failure is genuine rot to fix, not to suppress. Fails if the
  test reports zero links checked — assert the scanned-link count is non-zero.

### 24. Make the six multi-line write.md snippets lintable [S]
- **Files**: `claude/skills/tomlctl/references/write.md`
- **Depends on**: 4
- **Action**: Convert the six fenced snippets that `command_lint` currently skips as
  unbalanced-quoted into a form it can tokenise.
- **Detail**: Six fences open a single-quoted `--json '{` or `--ops '[` that closes on a later
  line, so a per-line tokeniser sees unbalanced quotes and the test prints them to a skip list
  rather than failing. Convert each to the `--json -` / `--ops -` heredoc form already
  demonstrated elsewhere in the same file, which is quote-balanced per line. There is a
  seventh multi-line block that is already a heredoc — leave it alone.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml` runs `command_lint` with an
  empty skip list; the test's skip-list output names no file. Fails if any snippet is deleted
  rather than converted — the file's documented invocation count must not drop.

### 25. Assert non-empty block extraction in verify-shared-blocks.sh [S]
- **Files**: `scripts/verify-shared-blocks.sh`
- **Depends on**: —
- **Action**: Fail the verifier when a named block extracts to nothing, instead of hashing the
  empty string and reporting parity OK.
- **Detail**: `hash_block` extracts between exact-equality marker lines and pipes straight to the
  hasher. Under a CR-preserving awk every block yields zero lines, both sides hash to
  `e3b0c442…b855`, and the script prints `shared-block parity: OK` and exits 0. The existing
  `grep -qF` marker guards do not catch this because they substring-match and so tolerate a
  trailing CR. Add a check between extraction and comparison that the captured block is
  non-empty, naming the file and block on failure.
- **Acceptance**: Running the script with `BINMODE=3` in the awk environment — which reproduces
  the CR-preserving case — exits non-zero and names the empty block, where today it exits 0 with
  `parity: OK`. Normal invocation still exits 0.

### Wave 3 — correctness and de-duplication

### 26. Widen the date-key promotion list [S]
- **Files**: `tomlctl/src/convert.rs`
- **Depends on**: 13
- **Action**: Add `promoted`, `dismissed` and `last_seen` to the date-key promotion list so
  backlog rows written through `items add` store native dates.
- **Detail**: Two sites must move together: the `DATE_KEYS` slice and the `is_date_key` jump
  table below it, which a runtime assertion already requires to agree. Consider whether
  `terminal_date` and `compacted_on` — also absent, also backlog date fields — belong in the same
  change; if they do, add them, and if not, say why in the commit.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml` passes, including the existing
  assertion that the slice and the jump table agree. A round-trip through `items add` writing a
  `promoted` key produces a native TOML date, not a quoted string.

### 27. Advertise the two missing flow verbs [S]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/README.md`
- **Depends on**: 11
- **Action**: Add `flow_list` and `flow_render_progress_log` to the `FEATURES` constant and the
  corresponding rows to the README feature table.
- **Detail**: `FEATURES` is a flat list of string literals; the `FlowOp` enum carries `List` and
  `RenderProgressLog` variants with no entry. The README table corresponds to `FEATURES` one-to-one
  in the same order and is maintained by hand, so add both rows in the matching position.
- **Acceptance**: `cargo test --manifest-path tomlctl/Cargo.toml` passes, and the
  `capabilities` output lists both new features. The README table's data-row count equals the
  `FEATURES` entry count — derive both rather than transcribing the numbers.

### 28. Make --json optional for an unset-only items update [M]
- **Files**: `tomlctl/src/cli/types.rs`, `tomlctl/src/cli/dispatch.rs`
- **Depends on**: 4, 10, 27
- **Action**: Allow `tomlctl items update <file> <id> --unset <field>` without a redundant
  `--json '{}'`.
- **Detail**: The `json` field on the update variant is a bare required `String`; make it
  optional and default to an empty patch when absent, then guard the dispatch site that
  unconditionally reads it. Require that at least one of `--json` or `--unset` is present, so an
  update naming neither still errors. Do **not** change the behaviour where a `null` or `[]`
  patch value is ignored rather than clearing a field — that is documented as intentional in
  `tomlctl/src/items.rs` and directs callers to `--unset`.
- **Acceptance**: `tomlctl items update <file> <id> --unset <field>` succeeds with no `--json`;
  an invocation with neither flag exits non-zero. Existing update tests in
  `cargo test --manifest-path tomlctl/Cargo.toml` still pass.

### 29. Correct the ids.rs collision doc comment [S]
- **Files**: `tomlctl/src/backlog/ids.rs`
- **Depends on**: 1
- **Action**: Rewrite the module doc comment so it describes what `add` actually does on a
  widening collision.
- **Detail**: The comment states that the caller re-derives the incumbent when a later-minted,
  lexicographically smaller `dedup_id` claims a short id an incumbent holds. No re-derivation
  exists — `derive_id` has one production call site and computes only the new row's id. The
  design deliberately freezes the incumbent's id so its evidence directory is never orphaned;
  document that reason, not the absent mechanism.
- **Acceptance**: The doc comment contains no claim of caller-side re-derivation.
  `cargo test --manifest-path tomlctl/Cargo.toml` passes — this is a comment-only change, so a
  behavioural diff means something else was edited.

### 30. Cover the ignored_set no-path-ignored branch [S]
- **Files**: `tomlctl/src/backlog/evidence_ops.rs`
- **Depends on**: 1
- **Action**: Add a test for the branch where `git check-ignore` exits 1 because nothing is
  ignored.
- **Detail**: Every existing sandbox writes the evidence ignore rules, so the probe always exits
  0 and this branch is unreached; only the process-failure path has coverage. Build a sandbox
  whose `.gitignore` lacks the backlog rules and assert the audit reports every evidence file as
  published rather than treating the exit code as an error.
- **Acceptance**: The new test fails if the exit-1 arm is changed to return the process-failure
  value. `cargo test --manifest-path tomlctl/Cargo.toml` passes.

### 31. Lift FIELD_LAST_UPDATED into backlog/schema.rs [M]
- **Files**: `tomlctl/src/backlog/schema.rs`, `tomlctl/src/backlog/add.rs`,
  `tomlctl/src/backlog/compact.rs`, `tomlctl/src/backlog/relate.rs`,
  `tomlctl/src/backlog/triage.rs`
- **Depends on**: 1
- **Action**: Replace the four private `FIELD_LAST_UPDATED` constants with one `pub(crate)`
  definition in the module that already holds every other field-name constant.
- **Detail**: Five files rather than three because the definition and all four call sites must
  land together to keep the tree compiling. Place the new constant beside the existing
  `FIELD_*` group in `tomlctl/src/backlog/schema.rs`, matching their visibility and ordering.
- **Acceptance**: `grep -rn 'const FIELD_LAST_UPDATED' tomlctl/src` returns exactly one line.
  `cargo build --manifest-path tomlctl/Cargo.toml` succeeds and
  `cargo test --manifest-path tomlctl/Cargo.toml` passes.

### 32. Promote strict_read_check to a single shared definition [M]
- **Files**: `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/json.rs`,
  `tomlctl/src/backlog/schema.rs`
- **Depends on**: 10, 18, 28, 31
- **Action**: Make `strict_read_check` `pub(crate)` and call it from the two places that
  currently re-implement it.
- **Detail**: The capture described three backlog read leaves each holding a copy; they in fact
  route through one inline copy in `read_store` in `tomlctl/src/backlog/schema.rs`, and the
  genuine second duplicate is a byte-identical private function in `tomlctl/src/json.rs` whose
  doc comment says it mirrors the original. Replace both. `read_store` returns an empty table
  rather than an error when not in strict mode — preserve that fallback exactly; the shared
  helper only performs the existence gate.
- **Acceptance**: `grep -rn 'fn strict_read_check' tomlctl/src` returns exactly one line.
  `cargo test --manifest-path tomlctl/Cargo.toml` passes, including the non-strict cases that
  rely on `read_store`'s empty-table fallback.

### 33. Share shipped_gitignore through the block verifier [M]
- **Files**: `tomlctl/src/test_support.rs`, `tomlctl/src/backlog/evidence_ops.rs`,
  `tomlctl/tests/common/mod.rs`, `scripts/shared-blocks.toml`
- **Depends on**: 25, 30
- **Note**: four files rather than three — the move, both marked regions and the manifest entry
  must land together or the verifier names a block that only one side carries.
- **Action**: Move the source-side `shipped_gitignore` copy into the crate's shared test-support
  module and gate the two remaining copies for byte-parity using the existing block verifier.
- **Detail**: `tomlctl/src/test_support.rs` is already declared `#[cfg(test)]` in
  `tomlctl/src/main.rs` and already described as the home for shared test scaffolding, so the
  helper and its two associated constants paste in at indent 0 with no per-item attributes —
  which is what makes byte-parity with the `tomlctl/tests/common/mod.rs` copy achievable. The
  copies are **not** identical today: the code lines match once indentation is stripped, but the
  doc comments differ in wording and wrap width, so unify the comments by hand first — rustfmt
  will not do it, since comment wrapping is nightly-only and off. Then wrap both regions in
  marker lines inside `/* … */` block comments (the verifier matches whole lines, which a block
  comment satisfies) and add one `[[block]]` entry naming the two files. Depends on task 25 so
  the verifier cannot pass this new block vacuously.
- **Acceptance**: `bash scripts/verify-shared-blocks.sh` exits 0 and reports the new block.
  Changing one character inside either marked region makes it exit non-zero.
  `cargo test --manifest-path tomlctl/Cargo.toml` passes.

### 34. Delete the vacuous blocks verify-skills verb [M]
- **Files**: `tomlctl/src/blocks.rs`, `tomlctl/src/cli/types.rs`,
  `tomlctl/src/cli/dispatch.rs`, `tomlctl/src/cli/dispatch/tests/skills.rs`,
  `claude/skills/tomlctl/references/flow.md`, `scripts/shared-blocks.toml`
- **Depends on**: 16, 23, 28, 32, 33
- **Note**: six files rather than three — removing a subcommand requires its enum variant,
  dispatch arm, engine, dormant test and documentation to go in one change or the crate does
  not compile.
- **Action**: Remove the `blocks verify-skills` subcommand, its drift engine, and its
  documentation.
- **Detail**: The verb iterates zero blocks — `scripts/shared-blocks.toml`'s two entries use only
  `name` and `files`, and the manifest's own prose already records `skill` as unused. Remove the
  engine and its normalisation helpers from `tomlctl/src/blocks.rs`, the enum variant and any
  `FEATURES` entry from `tomlctl/src/cli/types.rs`, the dispatch arm, and the collapsed "Old
  patterns" section documenting it at the tail of
  `claude/skills/tomlctl/references/flow.md`. Also drop the now-dangling explanation of the
  `skill` field from `scripts/shared-blocks.toml`, leaving the block entries themselves and the
  entry task 33 added. The verb has no `FEATURES` entry and no README row — verified at planning
  time — so `tomlctl/README.md` stays out of this task; confirm with a grep before assuming it
  still holds.
- **Acceptance**: `tomlctl blocks verify-skills` exits with an unknown-subcommand error.
  `grep -rn 'verify-skills' tomlctl/src claude/ scripts/` returns nothing.
  `cargo test --manifest-path tomlctl/Cargo.toml` passes and
  `bash scripts/verify-shared-blocks.sh` still exits 0.

## Dependency Graph

Scheduling is frontier-based; the per-task **Depends on** lines are authoritative. The waves
above are presentational.

— CHECKPOINT A after tasks 1-5: the crate is formatted, the clippy baseline is zero and denied
at the manifest, the `dispatch.rs` test module is split, and the fmt gate is armed. A buildable,
behaviour-preserving increment — and task 1's commit must be in history before task 6 can read
its SHA.

— CHECKPOINT B after tasks 6-20: blame provenance recorded and the finding-id sweep complete
across `tomlctl/src/` and the manifest. Comment-only edits plus one root file; the crate builds
and every test passes throughout.

— CHECKPOINT C after tasks 21-25: the four self-gating tests now cover what they claim, and the
shared-block verifier can no longer pass vacuously. Task 25 must land before task 33 arms a new
block.

— CHECKPOINT D after tasks 26-34: the two shipped bugs fixed, the three duplications collapsed,
and the vacuous verb removed.

## Risks

- **The plan touches far more than the ~25-file guidance** — task 1 alone rewrites 52 files, and
  the sweep adds 29 more. Mitigation: both are mechanical and verifiable by a single command;
  the sweep is partitioned so no task exceeds three files, and no task in it makes a semantic
  change. The guidance targets agent quality per task, which the partitioning preserves.

- **The finding-id sweep can silently corrupt test fixtures** — the same pattern matches
  `id = "R1"` literals in TOML fixture blocks, which are data. Mitigation: every sweep task is
  scoped to comment lines only and carries `cargo test` in its acceptance, so an altered fixture
  fails immediately rather than at review.

- **Tightening the two carrier tests may fail the real corpus** — tasks 21 and 22 make
  assertions stricter, and the existing carriers were written against the loose versions.
  Mitigation: each task's acceptance states that a corpus failure is genuine drift to report,
  not to accommodate by weakening the check. Expect to surface real gaps here.

- **133 finding-id comment hits under `tomlctl/tests/` are out of scope** — the sweep decision
  was taken against the measured `tomlctl/src/` figure, and `.claude/rules/documentation.md`
  globs `**/*.rs`, so the rule stays violated there. Mitigation: capture a backlog item for the
  `tomlctl/tests/` sweep rather than expanding this plan silently.

- **The reformat damages `git blame` for anyone who has not run the local config step** —
  `.git-blame-ignore-revs` is honoured automatically by GitHub but needs
  `git config blame.ignoreRevsFile` per clone. Mitigation: task 5 documents the step beside the
  existing `core.hooksPath` instruction in `CLAUDE.md`.

- **The fmt gate makes a missing `cargo` on the hook's PATH block every Rust commit** — the hook
  runs under `set -euo pipefail`. Mitigation: this matches the repository's existing posture,
  already documented as "a missing script is fatal"; the alternative report-only mode was
  considered and rejected as too easy to ignore.

- **Task 34's deletion is entangled with three other tasks on two files** — it edits
  `tomlctl/src/cli/types.rs` and `tomlctl/src/cli/dispatch.rs`, which tasks 27, 28 and 32 also
  touch. Mitigation: it depends on all of them, so it runs last and its edits apply to settled
  files; a `git status` conflict during the run means an edge was wrong and should stop the batch.
