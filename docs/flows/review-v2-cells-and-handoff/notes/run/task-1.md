Status: done — review cells.rs split into cells/ (mod, args, slicing, propose, append) with planned_cells shared helper

```toml run-note
files_touched = ["tomlctl/src/review/cells.rs", "tomlctl/src/review/cells/mod.rs", "tomlctl/src/review/cells/args.rs", "tomlctl/src/review/cells/slicing.rs", "tomlctl/src/review/cells/propose.rs", "tomlctl/src/review/cells/append.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass"

[[checks]]
command = "test ! -f tomlctl/src/review/cells.rs && test -f tomlctl/src/review/cells/{mod,args,slicing,propose,append}.rs"
result = "pass"

[[checks]]
command = "grep -q 'fn planned_cells' tomlctl/src/review/cells/mod.rs"
result = "pass"
```

## What changed

cells.rs was split by line range into the five files named in the spec; items shared across siblings were widened to pub(super). `planned_cells` lives in mod.rs and propose calls it. Child modules use `use super::*`. `review_cells::` tests (22) passed unchanged; clippy shows only two warnings in sibling-held fields.rs. Leaf files and mod.rs are rustfmt-clean.

One slip: I ran `git rm --cached` on cells.rs once and immediately undid it with `git reset -q -- <path>`; the index is back to HEAD, with cells.rs now deleted in the working tree only.
