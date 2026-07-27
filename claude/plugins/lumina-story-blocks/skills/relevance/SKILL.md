---
name: relevance
description: Set or supersede an epic/focus/story's relevance (active / backlog / deferred / rejected).
arguments: [work_item_id]
argument-hint: "[work_item_id]"
---

# `lumina:relevance`

Thin wrapper over `mcp__lumina__set_relevance`, kept for UI parity so the eventual lumina web UI drives the same skill the user invokes from `/lumina:relevance <id>`. Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§b/§c/§e.

```
mcp__lumina__set_relevance {
  id: "$work_item_id",
  relevance: "active" | "backlog" | "deferred" | "rejected"
}
```

Run the §b sequence over `detail.relevance`, with these skill-specific parts:

- **Kind-precondition** — `set_relevance` accepts `kind ∈ {epic, focus, story}` only; `task` and `project` are rejected server-side. Fail loud before any write: `set_relevance rejects kind=<kind>. Use this skill on an epic, focus, or story work item.`
- **The enum prompt** — `AskUserQuestion` body `Relevance is currently: <detail.relevance>. Change it?`; options `active` ("in flight now"), `backlog` ("queued, not started"), `deferred` ("paused on purpose"), `rejected` ("won't do"), `Keep current` ("leave unchanged"). `Keep current` counts as re-picking the existing value, so the §b step-4 no-op fires.
- **No-op line** — `relevance already set to <picked-value> — no change`.
- **Supersede substitutions** — `<field-name>` → `relevance`; `<current-value-summary>` → the existing relevance string (e.g. `backlog`).
- **§c summary line** — `relevance: set <work_item_id> to <new_value>` (first write); `relevance: superseded <work_item_id> from <old> to <new>` (supersession). `<work_item_id>` is the literal id.
- Do NOT read all of `attributes` and rewrite it as a JSON merge — `set_relevance` writes the dedicated `work_items.relevance` column, not an attribute key.
