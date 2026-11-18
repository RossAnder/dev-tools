Status: done — cold reader now reports open look choices, read-only-only UI acceptance and missing direction citations

```toml run-note
files_touched = ["claude/agents/plan-cold-reader.md"]

[[checks]]
command = "grep -qi 'look entry' ... && grep -qi 'layout' ... && grep -q 'direction.md' claude/agents/plan-cold-reader.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

`claude/agents/plan-cold-reader.md`: added three "What counts" entries under unactionable: an open layout/sizing/style/interaction choice, a user-visible behaviour with only read-only accept checks and no look entry or driving test, and a task citing no `design/direction.md` section or reading as a generic standalone implementation.
