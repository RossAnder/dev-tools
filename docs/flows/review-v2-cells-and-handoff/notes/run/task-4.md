Status: done — catalogue now has FileKind, the lockfiles routing key, lens kinds and the two new slice rules

```toml run-note
files_touched = ["tomlctl/src/review/catalogue.rs", "tomlctl/src/review/fields.rs", "claude/skills/review-v2-lenses/vocabulary.toml", "tomlctl/tests/review_vocabulary.rs"]

[[tangential]]
kind = "debt"
area = "tomlctl/src/review/fields.rs"
summary = "RoutingGlobs::kind and its lockfiles field are unused until kind-aware slicing lands, so clippy warns of dead code in the meantime"
why = "the gate's clippy step should run after the slicing task consumes them"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"

[[checks]]
command = "grep accept check for signal-evidence, changed-with-tests, lockfiles, fn kind"
result = "pass"
```

## What changed

- catalogue.rs: FileKind enum, SliceRule gains signal-evidence and changed-with-tests, Entry.kinds (a lens `kinds` key; `lockfile` refused with a schema error naming the lens file and id), Routing.lockfiles and the `[routing]` parser accepting it.
- fields.rs: RoutingGlobs takes a lockfiles set and gains `kind(path)` in priority lockfile, test, code, other; the test helper is updated.
- vocabulary.toml: the lockfiles list.
- review_vocabulary.rs: tests for the lockfiles key, an unknown routing key still refused, `kinds` parsing and the refused lockfile kind, and both new slice rules validating.

The nextest vocabulary check is the gate's and was not run.
