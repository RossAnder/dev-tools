Status: done — the engine skill commits on a gate acceptance, settles handover done-marks with their own commit, and the report prints the new gate and visual exceptions

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/SKILL.md", "claude/skills/implement-v2-engine/references/report.md"]

[[deviations]]
what = "The Visual pass step's skip condition also reads a gate acceptance for the last round, not only the Commit step and the Unstamped re-entry paragraph"
why = "The Commit step needs a passed visual pass or a visual accept for a milestone with ui_states, and the visual pass was skipped whenever the gate had not passed, so an accepted gate on a UI milestone could never reach its commit. The alternative, committing an accepted UI milestone without its visual pass, would skip the look check the user never waived."
means = "detail: the Commit step's skip condition and the Unstamped re-entry paragraph"

[[checks]]
command = "grep -q 'gate acceptance' claude/skills/implement-v2-engine/SKILL.md && grep -q 'accepted: <commands>' claude/skills/implement-v2-engine/references/report.md"
result = "pass"
```

## What changed

- `claude/skills/implement-v2-engine/SKILL.md`: rule 7 now says that whenever the user reports a handover performed, the orchestrator runs its `ready_check` when it has one, marks it with `tomlctl plan handover done <id> --slug <slug>`, records the observation as its `note` through `tomlctl plan handover set --slug <slug> --ndjson <path>`, and commits `plan.toml` and its sidecar by path in the same turn; the note and ready_check never hold a secret's value. Rule 8 names that done-mark commit as the one commit outside the two per milestone. The Commit step's skip condition, and the Visual pass step's, accept a `[[gate_accept]]` row of kind `accept` for the last gate round as a passed gate; a `base` row never satisfies it by itself. The Unstamped re-entry paragraph never re-gates a milestone holding such an acceptance, and each `extra-round` row raises the round cap by one.
- `claude/skills/implement-v2-engine/references/report.md`: under `gate:`, the lines `gate round <n> accepted: <commands>; backlog <B-id>`, `gate round <n>: one more round granted`, `base failure: <command> <ids>, <fixed|backlogged|waived>` and its `base`-record form `..., waived; backlog <B-id>`; beside the visual accept line, `visual round <r>: inspector disagreement on <states>, decided by the user`. All of them are listed among the `needs_person = false` lines.

Verbs and flags match the shipped references: `plan handover done`, `plan handover set --ndjson`, and `[[gate_accept]]` kinds `accept`, `extra-round` and `base` (claude/skills/tomlctl/references/plan.md and plan-run.md).

I did not run the `cli::dispatch::tests` subset. The view marks it `Run by: gate`.

D16's pre-flight offer of undone handovers on a finished flow belongs to run-milestone.md, which this task does not hold. A sibling task covers it.
