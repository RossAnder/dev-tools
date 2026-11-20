Status: done — external-effect mark and plan/needs-gate-effect added to write-and-prove.md

```toml run-note
files_touched = ["claude/skills/plan-v2-engine/references/write-and-prove.md"]

[[checks]]
command = "grep -q -- '--external-gate' write-and-prove.md"
result = "pass"

[[checks]]
command = "grep -q 'plan/needs-gate-effect' write-and-prove.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed
Three edits in write-and-prove.md: the `--external-gate` paragraph after the gate-probe paragraph (section 1), the `effect = "external"` sentence in the `run_by` bullet (section 3), and the `plan/needs-gate-effect` handling in section 4a.
