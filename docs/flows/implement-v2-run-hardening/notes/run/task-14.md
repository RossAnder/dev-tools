Status: done — the engine asks to accept a failed last visual round, records it with plan run visual, commits on it, reports it and skips it on re-entry

```toml run-note
files_touched = [
  "claude/skills/implement-v2-engine/references/visual.md",
  "claude/skills/implement-v2-engine/references/drain.md",
  "claude/skills/implement-v2-engine/SKILL.md",
  "claude/skills/implement-v2-engine/references/report.md",
]

[[checks]]
command = "grep -q 'plan run visual' claude/skills/implement-v2-engine/references/visual.md"
result = "pass"

[[checks]]
command = "grep -q 'accepted' claude/skills/implement-v2-engine/references/drain.md && grep -q 'visual_accept' claude/skills/implement-v2-engine/SKILL.md"
result = "pass"

[[checks]]
command = "grep -q 'accepted' claude/skills/implement-v2-engine/references/report.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"

[[checks]]
command = "tomlctl items list .claude/flows/implement-v2-run-hardening/plan.toml --array visual_accept --where milestone=B --where round=1 --get round"
result = "pass"
```

## What changed

- `references/visual.md`: step 2 now reads `[[visual_accept]]` for the current round with `tomlctl items list ... --array visual_accept --where milestone=<id> --where round=<r> --get round` before capturing; a row skips the pass and goes to the commit. "Two rounds at most" sends a last round that still holds a `fail` (an inspector's, or an `unsure` the orchestrator settled to `fail`) to the user first, and says an unsound run and a capture failure are never offered. A new section, "Accepting a failed last round", gives the `AskUserQuestion` (each failing state, its look criterion, the defect and its copied capture; two options naming their cost), states that the acceptance changes no verdict, note or round count, and the ordered steps: the backlog item by `backlog-capture` (check, then add, kind `debt`, the flow as `flow`), the payload staged in `<scratch>` and recorded with `plan run visual --accept --json '@<path>'` (payload keys per plan-run.md, including the `class = "unsure"` shape for a settled state), then the commit. "Re-entry" explains why an accepted round is never re-run.
- `references/drain.md`: step 10's entry condition, "Stamping on re-entry" and "Next" now commit on a passed pass or an accepted last round and leave an unaccepted failure uncommitted. The commit-paths, held-command and `--blocks` paragraphs milestone A's task wrote are untouched.
- `SKILL.md`: the visual-pass row's skip column and reference, the commit row, "Unstamped milestones", the visual.md reference summary and the user-question list name the acceptance and `[[visual_accept]]`.
- `references/report.md`: the `gate:` template gains `milestone <id> visual round <r> accepted: <states>; backlog <B-id>`, and the `needs_person = false` list names it (kind `gate` stays, per the section table).

## Notes

- `<r>` is the round of the pass that failed; since an accepted round mints no fix task, re-entry recomputes the same `<r>` from the `visual <id> round ` reasons and finds the row. The lookup command was checked against this flow's plan store (exits 0 with empty output when nothing matches).
- When `backlog check` finds an open item for the same defects, the prose takes that id instead of adding one, following the backlog-capture verdict ladder; `why` falls back to the picked option's label when the user gives no words.
- `visual.md` was listed among the siblings' files; it was unmodified when this task started, and task 15 (which also holds it) was pending, so the edit is this task's own. Task 15 will find these edits in place.
