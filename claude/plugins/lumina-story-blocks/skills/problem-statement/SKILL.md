---
name: problem-statement
description: Capture or update a story's problem_statement (what's broken, who's affected, success criteria).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:problem-statement`

Capture or update a story's `attributes.problem_statement` via `mcp__lumina__set_story_plan`: prompt along three axes, assemble the answers into one problem_statement string, and write it through lumina's merge-call story-plan setter. Also supports the round-5 framing scope-challenge the `plan-story` orchestrator's frame stage invokes (R51). Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e.

```
mcp__lumina__set_story_plan {
  id: "$work_item_id",
  problem_statement: "<assembled 3-axis text>"
}
```

`set_story_plan` is a merge call across `problem_statement` / `research_notes` / `execution_strategy`: pass ONLY `problem_statement` and the other two stay untouched. Never read them back to "preserve" them.

## The 3-axis prompt

Three questions in one `AskUserQuestion` call (one per axis, each with an `Other` free-text option so the user can type a substantive paragraph):

1. **What's broken or missing today?** — 1-2 sentences describing the concrete current-state pain point.
2. **Who's affected?** — 1 sentence naming the audience (end user / maintainer / external integrator / downstream consumer / etc.).
3. **What does success look like?** — 1-2 sentences naming the concrete observable outcome that would indicate the problem is solved.

Assemble the answers into this stable three-paragraph layout, so re-runs are byte-stable on the same answers and §b step 4's equality check has a stable string to compare:

```
What's broken: <answer 1>

Who's affected: <answer 2>

Success looks like: <answer 3>
```

## Framing scope-challenge (round-5, R51 — invoked from the orchestrator's frame stage)

The `plan-story` orchestrator's **frame** stage pushes back on whether the story is the right *size* and *ambition* before any approach is drafted. This skill supports that challenge but stays strictly **problem-only**: per §g.1 the solution shape lives in `execution_strategy` (owned by `/lumina:approach`), never here.

- Challenge concludes the problem is framed too NARROWLY → the user re-runs this skill and supersedes `problem_statement` with a broader framing (wider audience, larger "what's broken", more ambitious success criterion) via the §b step-5 path. `/lumina:user-interrogation`'s scope-challenge axis captures the open question ("should this be split / bigger?"); the *resolution* that changes the problem framing lands here.
- Challenge concludes the story should be SPLIT → the split is a work-item structure change (orchestrator / `decompose-tasks`), but each resulting story still gets its own narrowed `problem_statement` written here.
- Do NOT add solution/approach/sizing prose to `problem_statement` to "answer" the challenge — that is a §g.1 problem-overload violation.

No new tool or argument backs this: it is the existing 3-axis prompt + supersession path, re-invoked with a broadened/narrowed framing in mind.

## Skill-specific parts of the §b sequence

Run §b over `detail.attributes.problem_statement`, with:

- **Kind-precondition** — story-only. On any other kind, abort before any write: `problem-statement requires a story work item; got kind=<kind>.`
- **First write** — after assembling, return `problem_statement created on <work_item_id>.`
- **No-op line** — `problem_statement already matches the value you provided — no change.` (run the prompt and assemble first, then compare byte-for-byte).
- **Supersede substitutions** — `<field-name>` → `problem_statement`; `<current-value-summary>` → the first ~80 characters of the existing statement + `…` (single-line; collapse embedded newlines to spaces before truncating).
- **§c summary line** — `problem-statement: set on <work_item_id>` (first write); `problem-statement: superseded on <work_item_id>` (supersession, only when the prior value was non-null and the user chose `Replace`). `<work_item_id>` is the literal id.
