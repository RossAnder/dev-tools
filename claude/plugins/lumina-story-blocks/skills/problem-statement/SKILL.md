---
name: problem-statement
description: Capture or update a story's problem_statement (what's broken, who's affected, success criteria).
arguments: [work_item_id]
disable-model-invocation: true
---

# `lumina:problem-statement`

Capture or update a story's `attributes.problem_statement` via `mcp__lumina__set_story_plan`. The skill prompts the user along three axes (what's broken, who's affected, what success looks like), assembles the answers into a single problem_statement string, and writes it through lumina's merge-call story-plan setter.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency), §b-supersession (verbatim `AskUserQuestion` phrasing for the supersede prompt), §c (provenance recording via `record_task_activity`), §e (Sentry pattern — skill = instructions, MCP = execution).

## Target

`set_story_plan` accepts only `kind = story`. The lumina tool catalogue (`claude/skills/lumina/SKILL.md` §Definition tools) describes it as a merge call across `problem_statement` / `research_notes` / `execution_strategy` — passing ONLY `problem_statement` leaves the other two fields untouched (per §e, do NOT read the whole plan and rewrite it; pass only the field this skill owns). This skill fails loud at step 2 below if the caller passes a non-story id.

## MCP tool

```
mcp__lumina__set_story_plan {
  id: "$work_item_id",
  problem_statement: "<assembled 3-axis text>"
}
```

## The 3-axis prompt

The skill body asks the user three questions in one `AskUserQuestion` call (one question per axis, each with an `Other` free-text option so the user can type a substantive paragraph). The exact axes — taken from the parent plan §Approach §The 9 skills row for `problem-statement` — are:

1. **What's broken or missing today?** — 1-2 sentences describing the concrete current-state pain point. Example option label: `Free-text description (1-2 sentences)` with the user filling in via `Other`.
2. **Who's affected?** — 1 sentence naming the audience (end user / maintainer / external integrator / downstream consumer / etc.).
3. **What does success look like?** — 1-2 sentences naming the concrete observable outcome that would indicate the problem is solved.

After collecting the three answers, the skill assembles them into a single `problem_statement` string using this stable three-paragraph layout (so re-runs are byte-stable when the same answers are given):

```
What's broken: <answer 1>

Who's affected: <answer 2>

Success looks like: <answer 3>
```

The labelled-paragraph layout is deliberate — it makes the resulting prose self-documenting in the lumina UI, and the literal `What's broken:` / `Who's affected:` / `Success looks like:` prefixes give the equality check in §b step 4 a stable string to compare against.

## Body — 5-step check-before-act (per §b)

1. **Read**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.kind` and `detail.attributes.problem_statement` for the next steps.
2. **Precondition**: if `detail.kind != "story"`, abort with a one-line error: `"problem-statement requires a story work item; got kind=<kind>."` Do NOT call the tool.
3. **Absent → create**: if `detail.attributes.problem_statement` is null / absent / empty, run the 3-axis prompt above. Assemble the three answers into the layout shown. Call `set_story_plan({id: $work_item_id, problem_statement: <assembled>})`. Record provenance per §c with the `set` summary form. Return a one-line confirmation: `"problem_statement created on <work_item_id>."`
4. **Present and matches**: run the 3-axis prompt, assemble the new value, and compare against `detail.attributes.problem_statement`. If they match byte-for-byte, return the §b step-4 one-line confirmation: `"problem_statement already matches the value you provided — no change."`
5. **Present and differs**: invoke the §b-supersession `AskUserQuestion` template verbatim, substituting:
   - `<field-name>` → `problem_statement`
   - `<current-value-summary>` → the first ~80 characters of `detail.attributes.problem_statement` + `…` (single-line; replace any embedded newlines with spaces before truncating).
   On `Replace`, call `set_story_plan({id: $work_item_id, problem_statement: <new>})`, then record provenance per §c with the `superseded` summary form. On `Keep current`, abort the invocation without writing.

## Provenance recording (per §c)

After ANY successful write (step 3 first-create or step 5 supersession), append exactly one activity entry:

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "problem-statement: set on <work_item_id>",         # step 3
  # — or — for step 5 superseded:
  # summary: "problem-statement: superseded on <work_item_id>",
  body: "session=${CLAUDE_SESSION_ID}"
}
```

Use the `superseded` summary form only when the prior value was non-null and the user chose `Replace` in step 5. The `<work_item_id>` substitution is the literal id value (not the `$work_item_id` template).

## Sentry-pattern compliance (per §e)

The skill body decides which tool to call and what arguments to pass. Lumina's `set_story_plan` is a merge call — passing only `problem_statement` leaves `research_notes` and `execution_strategy` untouched. The skill body MUST NOT read those two fields and pass them back in to "preserve" them; the merge semantics handle that. Lumina's `repo.rs` also validates that the target is a story, runs the write in one transaction, and emits exactly one event drained to the git-export trail.
