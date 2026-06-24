---
name: problem-statement
description: Capture or update a story's problem_statement (what's broken, who's affected, success criteria).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:problem-statement`

Capture or update a story's `attributes.problem_statement` via `mcp__lumina__set_story_plan`. The skill prompts the user along three axes (what's broken, who's affected, what success looks like), assembles the answers into a single problem_statement string, and writes it through lumina's merge-call story-plan setter. It also supports the round-5 framing scope-challenge the `plan-story` orchestrator's frame stage invokes (R51) — strictly problem-only per §g.1; the solution shape stays in `execution_strategy` (see "Framing scope-challenge" below).

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency), §b-supersession (verbatim `AskUserQuestion` phrasing for the supersede prompt), §c (provenance recording via `record_task_activity`), §e (Sentry pattern — skill = instructions, MCP = execution).

## Target

`set_story_plan` accepts only `kind = story`. The lumina tool catalogue (`../mcp/SKILL.md` §Definition tools) describes it as a merge call across `problem_statement` / `research_notes` / `execution_strategy` — passing ONLY `problem_statement` leaves the other two fields untouched (per §e, do NOT read the whole plan and rewrite it; pass only the field this skill owns). This skill fails loud at the Precondition check below if the caller passes a non-story id.

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

## Framing scope-challenge (round-5, R51 — invoked from the orchestrator's frame stage)

The `plan-story` orchestrator's **frame** stage runs a scope-challenge (R51 devil's-advocate mandate) before locking the problem framing: it pushes back on whether the story is the right *size* and *ambition* before any approach is drafted. This skill SUPPORTS that challenge but stays strictly **problem-only** — per CONVENTIONS §g.1, the `problem_statement` describes the problem (what's broken / who's affected / success), and the *solution shape* lives in `execution_strategy` (owned by `/lumina:approach`), never here.

So when the frame stage challenges scope, this skill's role is to let the user **re-frame the problem itself** — broaden or narrow *what's broken*, *who's affected*, and *what success looks like* — NOT to fold in any approach or sizing-of-the-solution language. Concretely:

- When the frame-stage scope-challenge concludes the problem is framed too NARROWLY (R51's failure mode), the user re-runs this skill and supersedes the `problem_statement` with a broader problem framing (a wider audience, a larger "what's broken", a more ambitious success criterion) via the step-5 supersession path below. The companion `/lumina:user-interrogation` scope-challenge axis captures the open question ("should this be split / bigger?"); the *resolution* of that challenge that changes the problem framing lands here as a superseded `problem_statement`.
- When the challenge concludes the story should be SPLIT, the split itself is a work-item structure change (handled by the orchestrator / `decompose-tasks`), but each resulting story still gets its own narrowed `problem_statement` written here.
- This skill MUST NOT add solution/approach/sizing prose to `problem_statement` to "answer" the scope-challenge — that is a §g.1 violation (problem-overload). The challenge's *answer* about HOW to solve at the chosen scope belongs in `execution_strategy`; the challenge's effect on WHAT the problem is belongs here.

There is no new MCP tool or argument for this — the scope-challenge support is the existing 3-axis prompt + supersession path, re-invoked by the orchestrator with a broadened/narrowed problem framing in mind.

## Body — 5-step check-before-act (per §b)

**Precondition**: this skill applies only to `kind == "story"` work items. After step 1's `get_work_item` returns, verify `detail.kind == "story"`. If not, abort with a one-line error: `"problem-statement requires a story work item; got kind=<kind>."` Do NOT call any write tool. (This is a kind-guard, not a numbered §b step — the canonical sequence below preserves §b's 1-5 numbering exactly.)

1. **Read**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.kind` (consumed by the Precondition above) and proceed once the Precondition passes.
2. **Inspect field**: bind `detail.attributes.problem_statement` from the returned detail (may be null / absent / empty). This is the value against which the next three steps branch.
3. **Absent → create**: if `detail.attributes.problem_statement` is null / absent / empty, run the 3-axis prompt above. Assemble the three answers into the layout shown. Call `set_story_plan({id: $work_item_id, problem_statement: <assembled>})`. Record provenance per §c with the `set` summary form. Return a one-line confirmation: `"problem_statement created on <work_item_id>."`
4. **Present and matches**: run the 3-axis prompt, assemble the new value, and compare against `detail.attributes.problem_statement`. If they match byte-for-byte, return the §b step-4 one-line confirmation: `"problem_statement already matches the value you provided — no change."`
5. **Present and differs**: invoke the §b-supersession `AskUserQuestion` template verbatim, substituting:
   - `<field-name>` → `problem_statement`
   - `<current-value-summary>` → the first ~80 characters of `detail.attributes.problem_statement` + `…` (single-line; replace any embedded newlines with spaces before truncating).
   On `Replace`, call `set_story_plan({id: $work_item_id, problem_statement: <new>})`, then record provenance per §c with the `superseded` summary form. On `Keep current`, abort the invocation without writing.

## Provenance recording (per §c)

After ANY successful write (step 3 first-create or step 5 supersession), append exactly one activity entry per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §c. The `body`, `entry_type`, `origin`, and `work_item_id` fields are §c-canonical — see the §c template for the exact call shape.

Summary line: `"problem-statement: set on <work_item_id>"` for step 3 (first-create); `"problem-statement: superseded on <work_item_id>"` for step 5 (supersession). Use the `superseded` form only when the prior value was non-null and the user chose `Replace` in step 5. The `<work_item_id>` substitution is the literal id value (not the `$work_item_id` template).

## Sentry-pattern compliance (per §e)

The skill body decides which tool to call and what arguments to pass. Lumina's `set_story_plan` is a merge call — passing only `problem_statement` leaves `research_notes` and `execution_strategy` untouched. The skill body MUST NOT read those two fields and pass them back in to "preserve" them; the merge semantics handle that. Lumina's `repo.rs` also validates that the target is a story, runs the write in one transaction, and emits exactly one event drained to the git-export trail.
