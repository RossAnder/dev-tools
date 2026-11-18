Status: done — inputs contract now allows a plan worker's question and the orchestrator's relayed answer

```toml run-note
files_touched = ["claude/skills/flow-contract-user-inputs/SKILL.md"]

[[checks]]
command = "grep -q 'plan worker' ... && grep -q 'ledger plan\\|`plan` ledger\\|ledger = \"plan\"' ..."
result = "pass"
```

## What changed

`claude/skills/flow-contract-user-inputs/SKILL.md`: added the `plan` ledger to the field table (flow required, items are task ids), extended the `question` kind, added writer rows for the plan worker (one `add` of a `question`, own task id) and the `/implement-v2` orchestrator (`answer` relaying the user's own pick), and reworded the writer paragraph while keeping the rule that no agent answers, withdraws or acknowledges by its own judgement.
