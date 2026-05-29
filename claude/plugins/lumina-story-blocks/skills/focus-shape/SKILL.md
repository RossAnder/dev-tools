---
name: focus-shape
description: Set or supersede a focus's shape (vertical-slice / cross-cutting / foundational).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
disable-model-invocation: true
---

# `lumina:focus-shape`

Thin wrapper over `mcp__lumina__set_shape`. Sets a focus's `shape` discriminator so downstream planning and the lumina web UI can drive the same skill the user invokes from `/lumina:focus-shape <id>`.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter shape), §b (5-step check-before-act idempotency), §b-supersession (verbatim `AskUserQuestion` phrasing for the supersede prompt), §c (provenance recording via `record_task_activity`), §e (Sentry pattern — skill = instructions, MCP = execution), §f (no per-verb fragmentation), §m.2 (kind-precondition: focus-only writer fails-fast on the wrong kind; §m.2 is the on-point authority because §g/§h's taxonomy splits skills into any-kind lens writers and story-only column writers and has no epic/focus category).

## Target

`set_shape` is focus-only. Per the lumina tool catalogue (`../mcp/SKILL.md`), it rejects any other kind at the server. This skill fails loud at step 2 below if the caller passes a non-focus id, so the user sees a meaningful error rather than a generic `invalid_params` from the server.

## The three shapes (read this before invoking)

The user needs to know what they are choosing. Surface these one-line glosses in the option labels:

- **`vertical-slice`** — a thin end-to-end increment (touches every layer for one narrow capability).
- **`cross-cutting`** — a concern spanning many areas (a change that threads through the codebase rather than living in one slice).
- **`foundational`** — enabling groundwork (scaffolding/infrastructure that later focuses build on).

The skill body MUST surface these glosses in the option labels for step 3 / step 5 so the user picks meaningfully rather than guessing.

## MCP tool

```
mcp__lumina__set_shape {
  id: "$work_item_id",
  shape: "vertical-slice" | "cross-cutting" | "foundational"
}
```

## Body — 5-step check-before-act (per §b)

**Precondition**: this skill applies only to `kind == "focus"` work items (per §e's blessed local kind-check and the §m.2 kind-precondition rule). After step 1's `get_work_item` returns, verify `detail.kind == "focus"`. If not, abort with a one-line error: `"set_shape is focus-only — got kind=<kind>."` Do NOT call any write tool. (This is a kind-guard, not a numbered §b step — the canonical sequence below preserves §b's 1-5 numbering exactly.)

1. **Read**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.kind` (consumed by the Precondition above) and proceed once the Precondition passes.
2. **Inspect field**: bind `detail.shape` from the returned detail (may be null / absent). This is the value against which the next three steps branch.
3. **Absent → create**: if `detail.shape` is null / absent, ask the user via `AskUserQuestion` to pick one of `{vertical-slice, cross-cutting, foundational}` (one question, three options). Label each option with the gloss from "The three shapes" above. Call `set_shape({id: $work_item_id, shape: <picked>})`, then record provenance per §c, then return.
4. **Present and matches**: if `detail.shape` already equals the user's pick, return the §b step-4 confirmation: `"shape already matches the value you provided — no change."`
5. **Present and differs**: invoke the §b-supersession `AskUserQuestion` template verbatim, substituting:
   - `<field-name>` → `shape`
   - `<current-value-summary>` → the existing value (e.g. `vertical-slice`)
   On `Replace`, call `set_shape({id: $work_item_id, shape: <new>})`, then record provenance per §c. On `Keep current`, abort the invocation without writing.

## Provenance recording (per §c)

After any successful write (step 3 first-create or step 5 supersession), append one activity entry per [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §c. The `body`, `entry_type`, `origin`, and `work_item_id` fields are §c-canonical — see the §c template for the exact call shape (including the `${CLAUDE_SESSION_ID}` substitution guard).

Summary line: `"focus-shape: set <work_item_id> to <shape>"` for step 3 (first-create); `"focus-shape: superseded <work_item_id> from <old> to <new>"` for step 5 (supersession). The `<work_item_id>` substitution is the literal id value (not the `$work_item_id` template).

## Sentry-pattern compliance (per §e)

The skill body picks the value and calls the tool. Lumina's `set_shape` enforces the focus-only precondition, validates the shape enum, writes the dedicated `work_items.shape` column, and emits one event in the same transaction. The skill body MUST NOT attempt to read all of `attributes` and rewrite it as a JSON merge — `set_shape` writes a dedicated column, not an attribute key.
