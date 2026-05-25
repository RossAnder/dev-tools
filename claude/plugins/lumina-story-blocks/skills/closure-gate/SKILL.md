---
name: closure-gate
description: Set or supersede a story's closure_gate (hard / soft), controlling how unchecked acceptance criteria block child-task →done transitions.
arguments: [work_item_id]
disable-model-invocation: true
---

# `lumina:closure-gate`

Thin wrapper over `mcp__lumina__set_closure_gate`. Exists for UI parity (per the parent plan §Approach §The 9 skills — "Exists for UI parity (Q1)") so the eventual lumina web UI can drive the same skill the user invokes from `/lumina:closure-gate <id>`.

This skill cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md): §a (frontmatter), §b (5-step check-before-act), §b-supersession (verbatim `AskUserQuestion` phrasing), §c (provenance via `record_task_activity`), §e (Sentry pattern).

## Target

`set_closure_gate` is story-only. Per the lumina tool catalogue (`claude/skills/lumina/SKILL.md` §Planning & decision tools), it rejects any other kind at the server. This skill fails loud at step 2 below if the caller passes a non-story id.

## Behavioural effect (read this before invoking)

The user needs to know what they are choosing. Per the lumina catalogue:

- **`hard`**: each child task's →done transition is rejected while THAT task still has any unchecked acceptance criterion. The gate is the parent story's, applied per-task — checking a sibling task's AC does not help.
- **`soft`**: the transition is allowed, but the response flags the unchecked count for visibility. The story can close with partially-unchecked ACs.

The skill body MUST surface this distinction in the option labels for step 3 / step 5 so the user picks meaningfully rather than guessing.

## MCP tool

```
mcp__lumina__set_closure_gate {
  id: "$work_item_id",
  closure_gate: "hard" | "soft"
}
```

## Body — 5-step check-before-act (per §b)

1. **Read**: call `mcp__lumina__get_work_item({id: "$work_item_id"})`. Bind `detail.kind` and `detail.closure_gate`.
2. **Precondition**: if `detail.kind != "story"`, abort with a one-line error: `"set_closure_gate is story-only — got kind=<kind>."` Do NOT call the tool.
3. **Absent → create**: if `detail.closure_gate` is null / absent, ask the user via `AskUserQuestion` to pick one of `{hard, soft}` (one question, two options). Label the options with the per-task effect spelled out: `hard` → "block each child task's →done while it has unchecked ACs", `soft` → "allow →done, but flag the unchecked AC count". Call `set_closure_gate({id: $work_item_id, closure_gate: <picked>})`, then record provenance per §c, then return.
4. **Present and matches**: if `detail.closure_gate` already equals the user's pick, return the §b step-4 confirmation: `"closure_gate already set to <value> — no change."`
5. **Present and differs**: invoke the §b-supersession `AskUserQuestion` template verbatim, substituting:
   - `<field-name>` → `closure_gate`
   - `<current-value-summary>` → the existing value (e.g. `hard`)
   On `Replace`, call `set_closure_gate({id: $work_item_id, closure_gate: <new>})`, then record provenance per §c. On `Keep current`, abort the invocation without writing.

## Provenance recording (per §c)

After any successful write, append one activity entry:

```
mcp__lumina__record_task_activity {
  work_item_id: "$work_item_id",
  entry_type: "execution",
  origin: "plan",
  summary: "closure-gate: set <work_item_id> to <hard|soft>",
  body: "session=${CLAUDE_SESSION_ID}"
}
```

For step-5 supersession use `"closure-gate: superseded <work_item_id> from <old> to <new>"` instead.

## Sentry-pattern compliance (per §e)

The skill body picks the value and calls the tool. Lumina's `set_closure_gate` enforces the story-only precondition, writes the dedicated `work_items.closure_gate` column, and emits one event in the same transaction. The skill body MUST NOT model the `hard` task-blocking behaviour itself — that lives in lumina's task-transition logic (`repo.rs`'s `transition_status` path), not in the skill.
