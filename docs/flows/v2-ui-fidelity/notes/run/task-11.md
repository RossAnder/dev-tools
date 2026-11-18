Status: done — registered decision_class, ui_look, inputs_plan_ledger as features and required them in both carriers

```toml run-note
files_touched = ["tomlctl/src/cli/types/mod.rs", "tomlctl/tests/capabilities.rs", "tomlctl/README.md", "claude/commands/implement-v2.md", "claude/commands/plan-v2.md"]

[[checks]]
command = "grep -q 'ui_look' tomlctl/src/cli/types/mod.rs && grep -q 'decision_class' claude/commands/plan-v2.md && grep -q 'inputs_plan_ledger' claude/commands/implement-v2.md"
result = "pass"
```

## What changed

- FEATURES and the exact-count list in capabilities.rs gain the three names.
- README sample block and feature table gain the three names; the table descriptions are my wording.
- implement-v2.md pre-flight requires ui_look and inputs_plan_ledger; plan-v2.md requires decision_class and ui_look.

The capabilities nextest check is the gate's and was not run.
