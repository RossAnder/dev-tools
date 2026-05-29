---
name: epic-outcome
description: Capture or update an epic's outcome (what closing it delivers, who benefits, the observable signal it's achieved).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
---

# `lumina:epic-outcome`

Capture or update an epic's `attributes.outcome` via `mcp__lumina__set_epic_plan`. The skill prompts the user along three axes (what deliverable/intent closing this epic represents, why it matters / who benefits, what observable signal means it's achieved), assembles the answers into a single outcome string, and writes it through lumina's merge-call epic-plan setter.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency), §b-supersession (verbatim `AskUserQuestion` phrasing for the supersede prompt), §c (provenance recording via `record_task_activity`), §e (Sentry pattern — skill = instructions, MCP = execution), §m.2 (kind-precondition writers for the epic/focus fields — `epic-outcome` is the epic-only `outcome` writer, so it fails-fast on the wrong kind; §m.2 is the on-point authority because §g/§h's taxonomy splits skills into any-kind lens writers and story-only column writers and has no epic/focus category).

## Target

`set_epic_plan` accepts only `kind = epic`. It is a merge call across `outcome` / `context` — passing ONLY `outcome` leaves `context` untouched (per §e, do NOT read the whole plan and rewrite it; pass only the field this skill owns). This skill fails loud at the Precondition check below if the caller passes a non-epic id.

## MCP tool

```
mcp__lumina__set_epic_plan {
  id: "$work_item_id",
  outcome: "<assembled 3-axis text>"
}
```

## The 3-axis prompt

The skill body asks the user three questions in one `AskUserQuestion` call (one question per axis, each with an `Other` free-text option so the user can type a substantive paragraph). The axes are the epic-OUTCOME adaptation of the `problem-statement` interrogation:

1. **What does closing this epic deliver?** — 1-2 sentences naming the concrete deliverable or intent that "epic done" represents (the end-state capability, not the tasks that build it).
2. **Why does it matter / who benefits?** — 1 sentence naming the audience and the value (end user / maintainer / external integrator / downstream consumer / etc.) that closing the epic unlocks.
3. **What observable signal means it's achieved?** — 1-2 sentences naming the concrete, observable outcome that indicates the epic is genuinely done (the thing you could point at to say "this is finished").

After collecting the three answers, the skill assembles them into a single `outcome` string using this stable three-paragraph layout (so re-runs are byte-stable when the same answers are given):

```
Delivers: <answer 1>

Why it matters: <answer 2>

Observable signal: <answer 3>
```

The labelled-paragraph layout is deliberate — it makes the resulting prose self-documenting in the lumina UI, and the literal `Delivers:` / `Why it matters:` / `Observable signal:` prefixes give the equality check in §b step 4 a stable string to compare against.

## Body — 5-step check-before-act (per §b)

**Precondition**: this skill applies only to `kind == "epic"` work items (per §e's blessed local kind-check and the §m.2 kind-precondition rule for the epic-only `outcome` writer). After step 1's `get_work_item` returns, verify `detail.kind == "epic"`. If not, abort with a one-line error: `"epic-outcome requires an epic work item; got kind=<kind>."` Do NOT call any write tool. (This is a kind-guard, not a numbered §b step — the canonical sequence below preserves §b's 1-5 numbering exactly.)

1. **Read**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.kind` (consumed by the Precondition above) and proceed once the Precondition passes.
2. **Inspect field**: bind `detail.attributes.outcome` from the returned detail (may be null / absent / empty). This is the value against which the next three steps branch.
3. **Absent → create**: if `detail.attributes.outcome` is null / absent / empty, run the 3-axis prompt above. Assemble the three answers into the layout shown. Call `set_epic_plan({id: $work_item_id, outcome: <assembled>})`. Record provenance per §c with the `set` summary form. Return a one-line confirmation: `"outcome created on <work_item_id>."`
4. **Present and matches**: run the 3-axis prompt, assemble the new value, and compare against `detail.attributes.outcome`. If they match byte-for-byte, return the §b step-4 one-line confirmation: `"outcome already matches the value you provided — no change."`
5. **Present and differs**: invoke the §b-supersession `AskUserQuestion` template verbatim, substituting:
   - `<field-name>` → `outcome`
   - `<current-value-summary>` → the first ~80 characters of `detail.attributes.outcome` + `…` (single-line; replace any embedded newlines with spaces before truncating).
   On `Replace`, call `set_epic_plan({id: $work_item_id, outcome: <new>})`, then record provenance per §c with the `superseded` summary form. On `Keep current`, abort the invocation without writing.

## Provenance recording (per §c)

After ANY successful write (step 3 first-create or step 5 supersession), append exactly one activity entry per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §c. The `body`, `entry_type`, `origin`, and `work_item_id` fields are §c-canonical — see the §c template for the exact call shape (including the `${CLAUDE_SESSION_ID}` substitution guard).

Summary line: `"epic-outcome: set on <work_item_id>"` for step 3 (first-create); `"epic-outcome: superseded on <work_item_id>"` for step 5 (supersession). Use the `superseded` form only when the prior value was non-null and the user chose `Replace` in step 5. The `<work_item_id>` substitution is the literal id value (not the `$work_item_id` template).

## Sentry-pattern compliance (per §e)

The skill body decides which tool to call and what arguments to pass. Lumina's `set_epic_plan` is a merge call — passing only `outcome` leaves `context` untouched. The skill body MUST NOT read `context` and pass it back in to "preserve" it; the merge semantics handle that. Lumina's `repo.rs` also validates that the target is an epic, runs the write in one transaction, and emits exactly one event drained to the git-export trail.
