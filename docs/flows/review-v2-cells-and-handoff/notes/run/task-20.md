Status: done — checker predicate 4 and verifier Cleanup rule now bound the edit by fix_files

```toml run-note
files_touched = ["claude/agents/review-checker.md", "claude/agents/review-verifier.md"]

[[checks]]
command = "grep -q 'fix_files' claude/agents/review-checker.md && grep 'Cleanup' claude/agents/review-verifier.md | grep -q 'fix_files'"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

- review-checker.md predicate 4: edit paths may be file, target_file, an instance or a fix_files entry.
- review-verifier.md Cleanup: bound includes fix_files, and a behaviour-neutral change across those files is a cleanup to confirm.
