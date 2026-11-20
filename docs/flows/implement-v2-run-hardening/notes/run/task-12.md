Status: done — plan/ui-accept-structural now skips rows off the capture paths

```toml run-note
files_touched = ["tomlctl/src/plan/check.rs", "tomlctl/tests/plan_check.rs", "claude/skills/tomlctl/references/plan.md"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass"

[[checks]]
command = "grep -q 'fn ui_accept_structural_skips_a_row_off_the_capture_paths' tomlctl/tests/plan_check.rs"
result = "pass"

[[checks]]
command = "grep 'plan/ui-accept-structural' claude/skills/tomlctl/references/plan.md | grep -q 'exempt'"
result = "pass"
```

## What changed

- check.rs: `ui_findings` takes the capture globs (loaded once in `findings`; a malformed config or empty set applies no filter, the malformed warning stays with `ui_scope_findings`) and warns only when a row file matches.
- plan_check.rs: new test covering off-path, on-path and no-paths cases.
- plan.md: class row says a row touching no capture path is exempt.

The nextest test was not run (gate-owned).
