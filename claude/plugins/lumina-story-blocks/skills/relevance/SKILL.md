---
name: relevance
description: Set or supersede an epic/focus/story's relevance (active / backlog / deferred / rejected).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:relevance`

Thin wrapper over `mcp__lumina__set_relevance`. Exists for UI parity (per the parent plan §Approach §The 9 skills — "Exists for UI parity (Q1)") so the eventual lumina web UI can drive the same skill the user invokes from `/lumina:relevance <id>`.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency), §b-supersession (verbatim `AskUserQuestion` phrasing for the supersede prompt), §c (provenance recording via `record_task_activity`), §e (Sentry pattern — skill = instructions, MCP = execution).

## Target

`set_relevance` accepts only `kind ∈ {epic, focus, story}`. Per the lumina tool catalogue (`../mcp/SKILL.md` §Planning & decision tools), `task` and `project` rows are rejected by the MCP tool itself. This skill fails loud at step 2 below if the caller passes a task or project id, so the user sees a meaningful error rather than a generic `invalid_params` from the server.

## MCP tool

```
mcp__lumina__set_relevance {
  id: "$work_item_id",
  relevance: "active" | "backlog" | "deferred" | "rejected"
}
```

## Body — 5-step check-before-act (per §b)

1. **Read**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.kind` and `detail.relevance` for the next steps.
2. **Precondition**: if `detail.kind` is `"task"` or `"project"`, abort with a one-line error: `"set_relevance rejects kind=<kind>. Use this skill on an epic, focus, or story work item."` Do NOT call the tool.
3. **Absent → create**: if `detail.relevance` is null / absent, ask the user via `AskUserQuestion` to pick a relevance level. Surface the current value before asking. Question body: `Relevance is currently: <detail.relevance>. Change it?` Options: `active`, `backlog`, `deferred`, `rejected`, `Keep current` (with short rationale labels — e.g. `active` → "in flight now", `backlog` → "queued, not started", `deferred` → "paused on purpose", `rejected` → "won't do", `Keep current` → "leave unchanged"). If the user picks `Keep current`, treat it as if they re-picked the existing value — the equality check in step 4 will fire and the skill no-ops. Otherwise call `set_relevance({id: $work_item_id, relevance: <picked>})`, then record provenance per §c, then return.
4. **Present matches → no-op**: if the picked relevance equals the existing `detail.relevance`, return the §b step-4 one-line confirmation `relevance already set to <picked-value> — no change` and EXIT. Do not advance to step 5.
5. **Present and differs**: invoke the §b-supersession `AskUserQuestion` template verbatim, substituting:
   - `<field-name>` → `relevance`
   - `<current-value-summary>` → the existing relevance string (e.g. `backlog`)
   On `Replace`, call `set_relevance({id: $work_item_id, relevance: <new>})`, then record provenance per §c. On `Keep current`, abort the invocation without writing.

## Provenance recording (per §c)

After ANY successful write (step 3 first-create or step 5 supersession), append exactly one activity entry per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §c. The `body`, `entry_type`, `origin`, and `work_item_id` fields are §c-canonical — see the §c template for the exact call shape.

Summary line: `"relevance: set <work_item_id> to <new_value>"` for step 3 (first-create); `"relevance: superseded <work_item_id> from <old> to <new>"` for step 5 (supersession). Use the `superseded` form only when the prior value was non-null and the user chose `Replace` in step 5. The `<work_item_id>` substitution is the literal id value (not the `$work_item_id` template).

## Sentry-pattern compliance (per §e)

The skill body decides which tool to call and what arguments to pass. Lumina's `set_relevance` enforces the kind-precondition (epic/focus/story only), validates the relevance enum, and emits exactly one event in the same transaction. The skill body MUST NOT attempt to read all of `attributes` and rewrite it as a JSON merge — `set_relevance` writes the dedicated `work_items.relevance` column, not an attribute.
