Status: done — apply-rows, the task-store writer rule and the question loop now name the request procedure

```toml run-note
files_touched = [
  "claude/skills/implement-v2-engine/references/apply-rows.md",
  "claude/skills/flow-contract-task-store/SKILL.md",
  "claude/skills/implement-v2-engine/references/questions.md",
]

[[deviations]]
what = "The needs_files widening bullet in apply-rows.md covers any failed fix-row return with a non-empty needs_files, described as the route taken after a denied or unanswered scope request, rather than only one whose request record reads denied"
why = "A worker whose wait timed out returns needs_files the same way, and the drain cannot tell the two apart from the index; the tier move fails both for the same reason"
means = "detail: a fix row returning failed with needs_files is widened by drain.md's widening rules"

[[checks]]
command = "grep -q 'requests.md' claude/skills/implement-v2-engine/references/apply-rows.md && grep -q 'plan request add' claude/skills/flow-contract-task-store/SKILL.md && grep -q 'requests.md' claude/skills/implement-v2-engine/references/questions.md"
result = "pass"
```

## What changed

- `claude/skills/implement-v2-engine/references/apply-rows.md`, "Failure on an apply row": three bullets before **Tier**. A granted request is applied at the drain's Granted requests step and its `granted scope`/`granted amend` reason never counts toward `apply failure <n>`. A change item on a fix row that touches the milestone-wide `review reproduce-status` accept or moves the outcome off the finding being gone is denied. A `failed` return with `needs_files` is widened by drain.md step 6, "Widening a row from `needs_files`", park rules included, not moved up a tier (D2); its `needs_files from` reason is no apply failure either.
- `claude/skills/flow-contract-task-store/SKILL.md` §11: the v2 worker exception now also allows one `plan request add` per run, which writes to `plan.toml`, never `tasks.toml`; the orchestrator alone rules it and applies grants at the drain.
- `claude/skills/implement-v2-engine/references/questions.md`: step 2 says to rule every waiting `REQUEST` message by requests.md before opening the picker (D10). "The writes during a live run" names the request procedure's `plan.toml` writes too. The loop's steps and its never-answer rule are unchanged.

The dispatch-tests accept check is the gate's and was not run here.
