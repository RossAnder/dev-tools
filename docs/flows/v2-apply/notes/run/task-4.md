Status: done — `tomlctl review anchor <id> [--gone]` added, reusing `review add`'s `old` matching

## What changed

- `tomlctl/src/review/anchor.rs` (new): the verb. Loads the store read-only, finds the finding (NotFound error otherwise), and prints `{ok, id, kind, gone, reason}`. When the anchor does not hold it exits 1 after the print. It never writes the store or the tree.
  - Cleanup with `old`: `add::check_old` against `add::edit_target(file, target_file)`, which is the same call `review add` makes, with the same 20-line window around `line` and the same CRLF folding.
  - Cleanup without `old`: holds when `file` exists.
  - Defect: `file` exists and the symbol's last segment, taken after its last `::`, `.` or `/`, matches `crate::anchor::symbol_regex` (whole-word) over the file's bytes. An empty symbol skips that test.
  - `--gone`: refused (validation error, exit 1, nothing on stdout) unless the finding is a cleanup with `direction = "remove"` and an `old`. Otherwise exit 0 when `old` occurs nowhere in the whole edit target file, and 1 with the count when it does.
  - Advisory: refused (validation error).
- `tomlctl/src/review/add.rs`: pulled the edit-target choice out of `text_refusals` into `pub(super) fn edit_target`, and made `read_normalised` and `occurrences` `pub(super)`. `text_refusals` behaves as before, so `review add` accepts and refuses the same rows.
- `tomlctl/src/cli/types/review/read.rs`: `ReviewAnchorArgs` (positional `ID`, the store group, `--gone`, the read integrity flags).
- `tomlctl/src/cli/types/review/mod.rs`: the `ReviewOp::Anchor` variant.
- `tomlctl/src/review/dispatch.rs`: routes `Anchor` to `anchor::run`.
- `tomlctl/src/review/mod.rs`: `mod anchor;`. The `mod select;` line already in this file's diff comes from another task, and I left it alone.
- `tomlctl/tests/review_anchor.rs` (new): six integration tests:
  - a matching defect and cleanup hold, and the store bytes are unchanged afterwards;
  - drift exits 1: a missing symbol, `old` shifted out of the window, a renamed symbol, a deleted file;
  - a CRLF file matches;
  - `--gone` exits 1 while `old` is present or only shifted, and 0 once it is removed;
  - `--gone` is refused on a defect, an add and a change;
  - an advisory and an unknown id are refused.

## Deviations

- `anchor::run` takes `(target, id, gone, integrity)` rather than the `ReviewAnchorArgs` struct like the other verbs do. `crate::cli` re-exports the review argument types through an explicit list in `tomlctl/src/cli/mod.rs`, and `cli::types` is private. Naming the struct from `src/review/` would mean editing that file, which this task does not hold. The alternatives were to fail the task to get the file added, or to pass the fields. Passing the fields keeps the edit inside the task's files and costs nothing at runtime. A later edit to `cli/mod.rs` could switch to the struct.
- Choices the design left open:
  - An advisory is refused, because the design defines an anchor only for cleanups and defects.
  - Under `--gone`, an edit target file that no longer exists counts as gone (exit 0): `old` "occurs nowhere" in it.
  - A `remove` cleanup with no `old` is refused under `--gone`.

## Handover

None.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on anchor.rs, add.rs, dispatch.rs, read.rs and tests/review_anchor.rs: clean after applying it. `cli/types/review/mod.rs` and `review/mod.rs` declare `mod` children, so I left them to the gate.
- Accept check "the anchor verb exists" (`test -f tomlctl/src/review/anchor.rs`): passes.
- Accept check `cargo nextest run ... -E 'binary(review_anchor)'`: `Writes build: yes`, so I did not run it. It is the gate's.

TANGENTIAL: debt | tomlctl/src/cli/types/mod.rs | the capability list has no `review_anchor` entry for the new verb | a carrier pre-flight that gates on capabilities cannot detect an installed tomlctl that lacks `review anchor`
TANGENTIAL: debt | claude/skills/tomlctl/references/review.md | the review flag reference does not document `review anchor` or `--gone` | agents learn verbs from that reference, and the flag-table gate checks only what is documented
